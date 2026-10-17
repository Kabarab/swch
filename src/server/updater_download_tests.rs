use super::*;
use futures_util::stream;
use std::io;

fn state() -> Arc<Mutex<UpdateStatusResponse>> {
    UpdateManager::new().state
}

#[tokio::test]
async fn reports_partial_download_before_stream_finishes() {
    let state = state();
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("archive.zip");
    let (sender, receiver) = tokio::sync::mpsc::channel::<Vec<u8>>(4);
    let stream = stream::unfold(receiver, |mut receiver| async move {
        receiver.recv().await.map(|bytes| (Ok::<_, io::Error>(bytes), receiver))
    });
    let task_state = state.clone();
    let task_path = path.clone();
    let task = tokio::spawn(async move {
        write_download_stream(&task_state, stream, &task_path, Some(1000)).await
    });
    sender.send(vec![1; 250]).await.unwrap();
    tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            let st = state.lock().await.clone();
            if st.downloaded_bytes == 250 {
                assert_eq!(st.download_percent, Some(25.0));
                assert_eq!(st.total_bytes, Some(1000));
                assert!(st.bytes_per_second > 0);
                assert!(st.eta_seconds.is_some());
                assert!(!task.is_finished());
                break;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    }).await.unwrap();
    sender.send(vec![2; 750]).await.unwrap();
    drop(sender);
    task.await.unwrap().unwrap();
    assert_eq!(state.lock().await.download_percent, Some(100.0));
    let bytes = tokio::fs::read(path).await.unwrap();
    assert_eq!(&bytes[..250], &[1; 250]);
    assert_eq!(&bytes[250..], &[2; 750]);
}

#[tokio::test]
async fn unknown_length_reports_bytes_without_inventing_percentage() {
    for total in [None, Some(0)] {
        let state = state();
        let dir = tempfile::tempdir().unwrap();
        let stream = stream::iter([Ok::<_, io::Error>(vec![7; 42])]);
        write_download_stream(&state, stream, &dir.path().join("archive"), total).await.unwrap();
        let st = state.lock().await;
        assert_eq!(st.downloaded_bytes, 42);
        assert_eq!(st.total_bytes, None);
        assert_eq!(st.download_percent, None);
        assert_eq!(st.eta_seconds, None);
    }
}

#[tokio::test]
async fn rejects_empty_truncated_and_broken_downloads() {
    let dir = tempfile::tempdir().unwrap();
    for chunks in [vec![], vec![Ok(vec![0; 5])], vec![Err(io::Error::other("connection reset"))]] {
        let state = state();
        let result = write_download_stream(&state, stream::iter(chunks), &dir.path().join("archive"), Some(10)).await;
        assert!(result.is_err());
    }
}

#[tokio::test(start_paused = true)]
async fn stalled_download_reports_waiting_then_times_out() {
    let state = state();
    let dir = tempfile::tempdir().unwrap();
    let stream = stream::pending::<Result<Vec<u8>, io::Error>>();
    let task_state = state.clone();
    let path = dir.path().join("archive");
    let task = tokio::spawn(async move {
        write_download_stream(&task_state, stream, &path, Some(100)).await
    });
    // Let the asynchronous file open complete before advancing the idle clock.
    loop {
        if state.lock().await.total_bytes == Some(100) { break; }
        tokio::task::yield_now().await;
    }
    tokio::time::advance(Duration::from_secs(4)).await;
    tokio::task::yield_now().await;
    assert!(state.lock().await.message.contains("Ожидание"));
    assert_eq!(state.lock().await.bytes_per_second, 0);
    tokio::time::advance(DOWNLOAD_IDLE_TIMEOUT).await;
    assert!(task.await.unwrap().unwrap_err().contains("30 секунд"));
}

#[tokio::test]
async fn curl_retry_streams_progress_and_replaces_partial_archive() {
    use tokio::net::TcpListener;
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let (finish, release) = tokio::sync::oneshot::channel();
    let server = tokio::spawn(async move {
        let (mut socket, _) = listener.accept().await.unwrap();
        let mut request = [0; 4096];
        socket.read(&mut request).await.unwrap();
        socket.write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 1000\r\nConnection: close\r\n\r\n").await.unwrap();
        socket.write_all(&[1; 250]).await.unwrap();
        socket.shutdown().await.unwrap();
        drop(socket);
        let (mut socket, _) = listener.accept().await.unwrap();
        socket.read(&mut request).await.unwrap();
        socket.write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 1000\r\nConnection: close\r\n\r\n").await.unwrap();
        socket.write_all(&[2; 500]).await.unwrap();
        release.await.unwrap();
        socket.write_all(&[2; 500]).await.unwrap();
    });
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("archive");
    let state = state();
    let task_state = state.clone();
    let task_path = path.clone();
    let task = tokio::spawn(async move {
        download_archive(&task_state, &reqwest::Client::new(), &format!("http://{address}/archive"), &task_path, Some(1000)).await
    });
    tokio::time::timeout(Duration::from_secs(10), async {
        loop {
            let st = state.lock().await.clone();
            if st.downloaded_bytes == 500 {
                assert_eq!(st.download_percent, Some(50.0));
                assert_eq!(st.download_attempt, 2);
                assert!(st.message.contains("Повторная попытка"));
                assert!(!task.is_finished());
                break;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    }).await.unwrap();
    finish.send(()).unwrap();
    task.await.unwrap().unwrap();
    server.await.unwrap();
    assert_eq!(state.lock().await.download_percent, Some(100.0));
    assert_eq!(tokio::fs::read(path).await.unwrap(), vec![2; 1000]);
}

#[tokio::test]
async fn http_error_does_not_save_error_page_as_update() {
    use tokio::net::TcpListener;
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let server = tokio::spawn(async move {
        // Primary request and curl retry both receive a failing HTTP status.
        for _ in 0..2 {
            let (mut socket, _) = listener.accept().await.unwrap();
            let mut request = [0; 4096];
            let _ = socket.read(&mut request).await.unwrap();
            socket.write_all(b"HTTP/1.1 404 Not Found\r\nContent-Length: 5\r\nConnection: close\r\n\r\nerror").await.unwrap();
        }
    });
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("archive");
    let result = download_archive(&state(), &reqwest::Client::new(), &format!("http://{address}/archive"), &path, None).await;
    assert!(result.is_err());
    assert_eq!(tokio::fs::metadata(path).await.unwrap().len(), 0);
    server.abort();
}
