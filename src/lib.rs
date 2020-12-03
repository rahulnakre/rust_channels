use std::sync::{Arc, Condvar, Mutex};
// Vector with head and tail pointer
use std::collections::VecDeque;

pub struct Sender<T> {
  inner: Arc<Inner<T>>,
}

impl<T> Sender<T> {
  pub fn send(&mut self, t: T) {
    // Note the thing about the poisoned lock
    let mut queue = self.inner.queue.lock().unwrap();
    queue.push_back(t);
    // drop the lock so that whoever we notify can wake up immediately,
    // and we notify one thread
    drop(queue);
    self.inner.available.notify_one();
  }
}

// Needs a mutex cus a send and recv can happen at the same time
pub struct Receiver<T> {
  inner: Arc<Inner<T>>,
}

impl<T> Receiver<T> {
  // We're implementing a blocking receive, where
  // if there's nothing yet, it waits to receive something
  // unlike a try_recv, which would return Option<T>
  pub fn recv(&mut self) -> T {
    let mut queue = self.inner.queue.lock().unwrap();
    loop {
      match queue.pop_front() {
        Some(t) => return t, // return drops the mutex btw
        None => {
          // have to give up mutex when waiting
          queue = self.inner.available.wait(queue).unwrap();
        }
      }
    }
  }
}


// common thing in rust when multiple handles 
// that point to the same thing
// the inner type holds the data that is shared
// we use Mutex over a boolean semaphore bc
//  - semaphore would have to spin on the condition,
//    but Mutex's sleep/awake will be handled by the OS
struct Inner<T> {
  // receiver deal with the oldest message on channel first
  queue: Mutex<VecDeque<T>>,
  // condvar has to be outside the mutex btw, so sleeping 
  // threads can wake
  available: Condvar,
}

pub fn channel<T>() -> (Sender<T>, Receiver<T>) {
  let inner = Inner { 
    queue: Mutex::default(), 
    available: Condvar::default()
  };
  // shared inner
  let inner = Arc::new(inner);

  // return a (sender, receiver) of that inner
  (
    Sender {
      inner: inner.clone(),
    },
    Receiver {
      inner: inner.clone(),
    }
  )
}