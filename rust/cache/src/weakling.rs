use std::sync::{Arc, Weak};

use tokio::sync::Mutex;

#[derive(Debug)]
pub struct Weakling<T>(Arc<Mutex<Weak<T>>>);

// Derived `Clone` would demand `T: Clone`; cloning the handle never touches `T`.
impl<T> Clone for Weakling<T> {
    fn clone(&self) -> Self {
        Self(self.0.clone())
    }
}

impl<T> Weakling<T> {
    pub fn new(data: Arc<T>) -> Self {
        Self(Arc::new(Mutex::new(Arc::downgrade(&data))))
    }

    pub async fn upgrade(&self) -> Option<Arc<T>> {
        let lock = self.0.lock().await;
        lock.upgrade()
    }

    pub async fn fetch<F, Fut>(&self, f: F) -> Arc<T>
    where
        F: FnOnce() -> Fut,
        Fut: std::future::Future<Output = Arc<T>>,
    {
        let mut lock = self.0.lock().await;
        if let Some(data) = lock.upgrade() {
            data
        } else {
            let ret = f().await;
            *lock = Arc::downgrade(&ret);
            ret
        }
    }
}

impl<T> From<Weak<T>> for Weakling<T> {
    fn from(value: Weak<T>) -> Self {
        Self(Arc::new(Mutex::new(value)))
    }
}
