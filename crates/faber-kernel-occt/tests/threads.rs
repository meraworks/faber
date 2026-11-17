/// The OCCT thread pool is capped at six threads, even on a machine with more cores.
#[test]
fn occt_thread_pool_is_capped_at_six() {
    assert_eq!(faber_kernel_occt::thread_count(), 6);
}
