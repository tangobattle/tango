#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let client = nettai_client::Client::new("ws://localhost:9898", vec![]).await?;
    println!("{:?}", client.user_id().await);
    Ok(())
}
