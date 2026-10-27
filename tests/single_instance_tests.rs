use std::net::SocketAddr;
use tokio::net::TcpListener;

#[tokio::test]
async fn test_single_instance_port_conflict_detection() {
    // Bind ephemeral port for the first instance
    let host = [127, 0, 0, 1];
    let initial_listener = TcpListener::bind(SocketAddr::from((host, 0)))
        .await
        .expect("First listener should bind successfully");
    let bound_addr = initial_listener
        .local_addr()
        .expect("Must retrieve bound local address");

    // Attempt to bind second instance to the exact same address
    let second_attempt = TcpListener::bind(bound_addr).await;

    match second_attempt {
        Err(e) if e.kind() == std::io::ErrorKind::AddrInUse => {
            // Expected: AddrInUse is detected cleanly
        }
        other => {
            panic!(
                "Expected AddrInUse error when binding to already bound port {:?}, got {:?}",
                bound_addr, other
            );
        }
    }

    // Drop the initial listener (simulating process termination)
    drop(initial_listener);

    // After first instance releases the port, a new instance must be able to bind
    let third_attempt = TcpListener::bind(bound_addr).await;
    assert!(
        third_attempt.is_ok(),
        "Port should be re-bindable after previous instance terminates"
    );
}
