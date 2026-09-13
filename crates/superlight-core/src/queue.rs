#[derive(Clone, Debug)]
pub struct BoundedQueue<T: Copy, const N: usize> {
    items: [Option<T>; N],
    head: usize,
    len: usize,
}

impl<T: Copy, const N: usize> Default for BoundedQueue<T, N> {
    fn default() -> Self {
        Self {
            items: [None; N],
            head: 0,
            len: 0,
        }
    }
}

impl<T: Copy, const N: usize> BoundedQueue<T, N> {
    pub fn push(&mut self, item: T) -> Result<(), T> {
        if self.len == N {
            return Err(item);
        }
        self.items[(self.head + self.len) % N] = Some(item);
        self.len += 1;
        Ok(())
    }

    pub fn pop(&mut self) -> Option<T> {
        if self.len == 0 {
            return None;
        }
        let item = self.items[self.head].take();
        self.head = (self.head + 1) % N;
        self.len -= 1;
        item
    }

    pub fn clear(&mut self) {
        self.items.fill(None);
        self.head = 0;
        self.len = 0;
    }

    pub fn len(&self) -> usize {
        self.len
    }
    pub fn is_empty(&self) -> bool {
        self.len == 0
    }
    pub const fn capacity(&self) -> usize {
        N
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bounded_fifo_across_wraparound() {
        let mut queue = BoundedQueue::<u32, 3>::default();
        for round in 0..100_000 {
            assert!(queue.push(round).is_ok());
            assert!(queue.push(round + 1).is_ok());
            assert!(queue.push(round + 2).is_ok());
            assert_eq!(queue.push(round + 3), Err(round + 3));
            assert_eq!(queue.len(), 3);
            assert_eq!(queue.pop(), Some(round));
            assert_eq!(queue.pop(), Some(round + 1));
            assert_eq!(queue.pop(), Some(round + 2));
            assert!(queue.pop().is_none());
        }
    }

    #[test]
    fn zero_capacity_is_safe() {
        let mut queue = BoundedQueue::<u8, 0>::default();
        assert_eq!(queue.push(1), Err(1));
        assert_eq!(queue.pop(), None);
    }
}
