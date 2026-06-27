//! LWP message buffer for defragmenting BLE notifications.

use tracing::warn;

/// Maximum LWP message length we will accept. Real LWP messages are well under
/// this; a larger length byte means the stream is desynced or the peer is
/// misbehaving — drop the buffer and resync rather than grow unbounded.
const MAX_MESSAGE_LEN: usize = 64;

/// Accumulates incoming BLE notification bytes and extracts complete LWP
/// messages based on the length prefix in byte 0.
#[derive(Debug, Default)]
pub struct MessageBuffer {
    buffer: Vec<u8>,
}

impl MessageBuffer {
    pub fn new() -> Self {
        Self {
            buffer: Vec::with_capacity(MAX_MESSAGE_LEN),
        }
    }

    /// Append `data` and return any complete messages that are now available.
    pub fn push(&mut self, data: &[u8]) -> Vec<Vec<u8>> {
        self.buffer.extend_from_slice(data);
        self.extract_messages()
    }

    fn extract_messages(&mut self) -> Vec<Vec<u8>> {
        let mut messages = Vec::new();
        loop {
            if self.buffer.is_empty() {
                break;
            }
            let len = self.buffer[0] as usize;
            if len == 0 || len > MAX_MESSAGE_LEN {
                warn!(len, "Invalid LWP length byte; clearing buffer to resync");
                self.buffer.clear();
                break;
            }
            if self.buffer.len() < len {
                break;
            }
            messages.push(self.buffer.drain(..len).collect());
        }
        messages
    }

    /// Discard buffered bytes (call on disconnect).
    pub fn clear(&mut self) {
        self.buffer.clear();
    }

    #[cfg(test)]
    pub fn has_pending(&self) -> bool {
        !self.buffer.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    mod message_buffer {
        use super::*;

        #[test]
        fn empty_buffer() {
            let buffer = MessageBuffer::new();
            assert!(!buffer.has_pending());
        }

        #[test]
        fn single_complete_message() {
            let mut buffer = MessageBuffer::new();
            let data = vec![0x06, 0x00, 0x01, 0x06, 0x05, 50];
            let messages = buffer.push(&data);
            assert_eq!(messages.len(), 1);
            assert_eq!(messages[0], data);
            assert!(!buffer.has_pending());
        }

        #[test]
        fn fragmented_message_two_parts() {
            let mut buffer = MessageBuffer::new();
            let messages1 = buffer.push(&[0x06, 0x00, 0x01]);
            assert!(messages1.is_empty());
            assert!(buffer.has_pending());
            let messages2 = buffer.push(&[0x06, 0x05, 50]);
            assert_eq!(messages2.len(), 1);
            assert_eq!(messages2[0], vec![0x06, 0x00, 0x01, 0x06, 0x05, 50]);
            assert!(!buffer.has_pending());
        }

        #[test]
        fn multiple_messages_in_one_packet() {
            let mut buffer = MessageBuffer::new();
            let mut data = vec![0x06, 0x00, 0x01, 0x06, 0x05, 50];
            data.extend_from_slice(&[0x05, 0x00, 0x82, 0x32, 0x02]);
            let messages = buffer.push(&data);
            assert_eq!(messages.len(), 2);
            assert_eq!(messages[0], vec![0x06, 0x00, 0x01, 0x06, 0x05, 50]);
            assert_eq!(messages[1], vec![0x05, 0x00, 0x82, 0x32, 0x02]);
            assert!(!buffer.has_pending());
        }

        #[test]
        fn multiple_messages_with_trailing_fragment() {
            let mut buffer = MessageBuffer::new();
            let mut data = vec![0x06, 0x00, 0x01, 0x06, 0x05, 50];
            data.extend_from_slice(&[0x05, 0x00]);
            let messages = buffer.push(&data);
            assert_eq!(messages.len(), 1);
            assert!(buffer.has_pending());
            let messages2 = buffer.push(&[0x82, 0x32, 0x02]);
            assert_eq!(messages2.len(), 1);
            assert_eq!(messages2[0], vec![0x05, 0x00, 0x82, 0x32, 0x02]);
        }

        #[test]
        fn clear_buffer() {
            let mut buffer = MessageBuffer::new();
            buffer.push(&[0x06, 0x00, 0x01]);
            assert!(buffer.has_pending());
            buffer.clear();
            assert!(!buffer.has_pending());
        }

        #[test]
        fn zero_length_message_ignored() {
            let mut buffer = MessageBuffer::new();
            let data = vec![0x00, 0x06, 0x00, 0x01, 0x06, 0x05, 50];
            let messages = buffer.push(&data);
            assert!(messages.is_empty());
            assert!(!buffer.has_pending());
        }

        #[test]
        fn oversized_length_clears_buffer() {
            let mut buffer = MessageBuffer::new();
            let data = vec![0xFF, 0x01, 0x02, 0x03];
            let messages = buffer.push(&data);
            assert!(messages.is_empty());
            assert!(!buffer.has_pending());
        }

        #[test]
        fn recovers_after_oversized_length() {
            let mut buffer = MessageBuffer::new();
            buffer.push(&[0xFF, 0x01]);
            assert!(!buffer.has_pending());
            let messages = buffer.push(&[0x06, 0x00, 0x01, 0x06, 0x05, 50]);
            assert_eq!(messages.len(), 1);
        }

        #[test]
        fn byte_by_byte_assembly() {
            let mut buffer = MessageBuffer::new();
            let full_message = vec![0x06, 0x00, 0x01, 0x06, 0x05, 75];
            for (i, byte) in full_message.iter().enumerate() {
                let messages = buffer.push(&[*byte]);
                if i < full_message.len() - 1 {
                    assert!(messages.is_empty());
                    assert!(buffer.has_pending());
                } else {
                    assert_eq!(messages.len(), 1);
                    assert_eq!(messages[0], full_message);
                }
            }
        }
    }
}
