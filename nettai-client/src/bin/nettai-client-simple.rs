#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let client = nettai_client::Client::new("ws://localhost:9898", vec![]).await?;
    loop {
        let user_id = if let Some(user_id) = client.user_id().await {
            user_id
        } else {
            continue;
        };

        println!("{:?}", user_id);
    }
    Ok(())
}
