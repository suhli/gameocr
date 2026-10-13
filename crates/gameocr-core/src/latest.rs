//! A single latest-frame slot. Producers can never build an unbounded queue.
#[derive(Debug)]
pub struct Latest<T> {
    value: Option<T>,
    pub dropped: u64,
}
impl<T> Default for Latest<T> {
    fn default() -> Self {
        Self {
            value: None,
            dropped: 0,
        }
    }
}
impl<T> Latest<T> {
    pub fn replace(&mut self, value: T) {
        if self.value.replace(value).is_some() {
            self.dropped += 1;
        }
    }
    pub fn take(&mut self) -> Option<T> {
        self.value.take()
    }
    pub fn peek(&self) -> Option<&T> {
        self.value.as_ref()
    }
    pub fn clear(&mut self) {
        self.value = None;
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn overload_only_keeps_latest() {
        let mut q = Latest::default();
        for i in 0..10000 {
            q.replace(i);
        }
        assert_eq!(q.dropped, 9999);
        assert_eq!(q.take(), Some(9999));
        assert_eq!(q.take(), None);
    }
}
