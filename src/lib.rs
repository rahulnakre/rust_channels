use std::sync::{Arc, Condvar, Mutex};

pub struct Sender<T> {
  inner: Arc<Inner<T>>,
}

// Needs a mutex cus a send and recv can happen at the same time
pub struct Receiver<T> {
  inner: Arc<Inner<T>>,
}

// common thing in rust when multiple handles 
// that point to the same thing
// the inner type holds the data that is shared
// we use Mutex over a boolean semaphore bc
//  - semaphore would have to spin on the condition,
//    but Mutex's sleep/awake will be handled by the OS
struct Inner<T> {
  // receiver deal with the oldest message on channel first
  queue: Mutex<Vec<T>>,
}

pub fn channel<T>() -> (Sender<T>, Receiver<T>) {
  let inner = Inner { queue: Mutex::default() };
  // shared inner
  let inner = Arc::new(Mutex::new(inner));

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