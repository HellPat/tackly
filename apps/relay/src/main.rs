use std::{env, net::SocketAddr};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let database_url =
        env::var("DATABASE_URL").unwrap_or_else(|_| "sqlite://tackly-relay.db".into());
    let pool = tackly_relay::connect(&database_url).await?;
    tackly_relay::migrate(&pool).await?;
    if env::args().nth(1).as_deref() == Some("migrate") {
        println!("migrations applied");
        return Ok(());
    }
    tackly_relay::record_restart(&pool).await?;
    let address: SocketAddr = env::var("TACKLY_BIND")
        .unwrap_or_else(|_| "127.0.0.1:3000".into())
        .parse()?;
    let listener = tokio::net::TcpListener::bind(address).await?;
    println!("tackly sync listening on {address}");
    axum::serve(listener, tackly_relay::router(pool)).await?;
    Ok(())
}
