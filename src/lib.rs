#![feature(test)]

use std::sync::{Arc, Condvar, Mutex};
// Vector with head and tail pointer
use std::collections::VecDeque;
use std::thread;
use std::sync::mpsc;

extern crate test;

pub struct Sender<T> {
  shared: Arc<Shared<T>>,
}

/**
 * Need to have Sender be cloneable, but #[derive(Clone)] doesn't work,
 * as it desugars to:
 * impl<T: Clone> Clone for Sender<T> {
 *  fn clone(&self) -> Self {
 *    // ....
 * }
 * } 
 *  which adds the Clone bound to T as well.
 * In our case, Arc implements Clone even if T doesn't implement Clone
 * 
*/

impl<T> Clone for Sender<T> {
  fn clone(&self) -> Self {
    // cus we're keeping track of # of senders,
    // we need to update the count when cloning
    let mut inner = self.shared.inner.lock().unwrap();
    inner.senders += 1;
    drop(inner);


    Sender {
      // dont do this
      // shared: self.shared.clone(),
      // we do this to say we want to clone the Arc, 
      // not the thing inside
      shared: Arc::clone(&self.shared),
    }
  }
}

impl<T> Drop for Sender<T> {
  fn drop(&mut self) {
    let mut inner = self.shared.inner.lock().unwrap();
    inner.senders -= 1;
    let was_last = inner.senders == 0;
    drop(inner);
    if was_last {  
      self.shared.available.notify_one();
    }
  }
}


impl<T> Sender<T> {
  pub fn send(&mut self, t: T) {
    // Note the thing about the poisoned lock
    let mut inner = self.shared.inner.lock().unwrap();
    inner.queue.push_back(t);
    // drop the lock so that whoever we notify can wake up immediately,
    // and we notify one thread
    drop(inner);
    self.shared.available.notify_one();
  }
}

// Needs a mutex cus a send and recv can happen at the same time
pub struct Receiver<T> {
  shared: Arc<Shared<T>>,
  buffer: VecDeque<T>,
}

impl<T> Receiver<T> {
  // We're implementing a blocking receive, where
  // if there's nothing yet, it waits to receive something
  // unlike a try_recv, which would return Option<T>
  // Note: while trying to deal with the 0 senders case,
  // we do end up needing an Option<T> cus it can be none
  // if a Receiver is still blocking while senders r gone
  pub fn recv(&mut self) -> Option<T> {
    // for batch recv optimization: *
    // if there's some leftover items from the last time we took the lock
    if let Some(t) = self.buffer.pop_front() {
      return Some(t)
    }
    let mut inner = self.shared.inner.lock().unwrap();
    loop {
      match inner.queue.pop_front() {
        // return drops the mutex btw
        Some(t) => {
          // for batch recv optimization: *
          // we only have 1 receiver, so anytime we do get the lock,
          // might as well pick off everything from the queue at once
          if !inner.queue.is_empty() {
            // swap that vedeque w this new vecdeque
            std::mem::swap(&mut self.buffer, &mut inner.queue)
          }
          return Some(t)
        }
        // None if dbg!(inner.senders) == 0 => return None,
        None if inner.senders == 0 => return None,
        None => {
          // only if # of senders > 0 do we end up here
          // have to give up mutex when waiting
          inner = self.shared.available.wait(inner).unwrap();
        }
      }
    }
  }
}


impl<T> Iterator for Receiver<T> {
  type Item = T;
  
  fn next(&mut self) -> Option<Self::Item> {
    self.recv()
  }

}

struct Inner<T> {
  queue: VecDeque<T>,
  senders: usize,
}

// common thing in rust when multiple handles 
// that point to the same thing
// the shared (inner) type holds the data that is shared
// we use Mutex over a boolean semaphore bc
//  - semaphore would have to spin on the condition,
//    but Mutex's sleep/awake will be handled by the OS
struct Shared<T> {
  // receiver deal with the oldest message on channel first
  inner: Mutex<Inner<T>>,
  // condvar has to be outside the mutex btw, so sleeping 
  // threads can wake
  available: Condvar,
}

pub fn channel<T>() -> (Sender<T>, Receiver<T>) {
  let inner = Inner {
    queue: VecDeque::default(),
    senders: 1,
  };

  let shared = Shared { 
    inner: Mutex::new(inner), 
    available: Condvar::new()
  };
  // shared 
  let shared = Arc::new(shared);

  // return a (sender, receiver) of that shared
  (
    Sender {
      shared: shared.clone(),
    },
    Receiver {
      shared: shared.clone(),
      buffer: VecDeque::default(),
    }
  )
}

#[cfg(test)]
mod tests {
  use super::*;
  use test::Bencher;

  #[test]
  fn ping_pong() {
    let (mut tx, mut rx) = channel();
    tx.send(42);
    assert_eq!(rx.recv(), Some(42))
  }

  // this hangs
  // need a way to tell receivers that all
  // senders are closed
  #[test]
  fn closed_tx() {
    let (tx, mut rx) = channel::<()>();
    // doesnt drop tx for some reason
    // let _ = tx;
    drop(tx);
    assert_eq!(rx.recv(), None);
  }
  
  #[test]
  fn closed_rx() {
    let (_tx, rx) = channel::<()>();
    drop(rx);
    // tx.send(42);
  }

  fn my_channel_send_main() {
    let (mut tx, mut rx) = channel();
    tx.send(10);
    // assert_eq!(rx.recv(), Some(10));
  }

  #[bench]
  fn my_channel_send(b: &mut Bencher) {
    b.iter(|| {
      my_channel_send_main();
    });
  }

  fn mpsc_channel_send_main() {
    let (tx, rx) = mpsc::channel();
    tx.send(10);
    // assert_eq!(rx.recv(), Some(10));
  }

  #[bench]
  fn mpsc_channel_send(b: &mut Bencher) {
    b.iter(|| {
      mpsc_channel_send_main();
    });
  }



  fn play2_main() {
    let (tx, mut rx): (Sender<i32>, Receiver<i32>) = channel();  
    let mut children = Vec::new();
    static NTHREADS: i32 = 30;

    for id in 0..NTHREADS {
      let mut thread_tx = tx.clone();

      let child = thread::spawn(move || {
        thread_tx.send(id);

        println!("Thread {} done", id);
      });

      children.push(child);
    }

    // messages collected
    let mut ids = Vec::with_capacity(NTHREADS as usize);
    for _ in 0..NTHREADS {
      ids.push(rx.recv());
    }

    for child in children {
      child.join().expect("child thread panicked");
    }

    println!("{:?}", ids);
  }

  #[bench]
  fn play2(b: &mut Bencher) {
    b.iter(|| {
      play2_main();
    })
  }


}