#[test]
fn websocket_buffers_are_explicitly_bounded() {
    let c = cc_desk_remote_transport::websocket_config();
    assert_eq!(c.max_message_size, Some(65536));
    assert_eq!(c.max_frame_size, Some(65536));
    assert!(c.write_buffer_size < c.max_write_buffer_size);
    assert!(c.max_write_buffer_size <= 131072);
    assert!(!c.accept_unmasked_frames);
}
