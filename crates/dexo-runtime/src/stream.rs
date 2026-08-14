pub fn bounded_events<T>(
    capacity: usize,
) -> (tokio::sync::mpsc::Sender<T>, tokio::sync::mpsc::Receiver<T>) {
    assert!(capacity > 0);
    tokio::sync::mpsc::channel(capacity)
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::bounded_events;

    #[tokio::test]
    async fn producer_waits_when_two_batches_are_buffered() {
        let (producer, mut consumer) = bounded_events::<u8>(2);
        producer.send(1).await.unwrap();
        producer.send(2).await.unwrap();
        let blocked = tokio::time::timeout(Duration::from_millis(20), producer.send(3)).await;
        assert!(blocked.is_err());
        assert_eq!(consumer.recv().await, Some(1));
    }
}
