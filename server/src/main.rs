use std::{env, net::SocketAddr};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let database_url =
        env::var("DATABASE_URL").unwrap_or_else(|_| "sqlite://tackly-sync.db".into());
    let pool = tackly_sync::connect(&database_url).await?;
    tackly_sync::migrate(&pool).await?;
    if env::args().nth(1).as_deref() == Some("migrate") {
        println!("migrations applied");
        return Ok(());
    }
    let address: SocketAddr = env::var("TACKLY_BIND")
        .unwrap_or_else(|_| "127.0.0.1:3000".into())
        .parse()?;
    let listener = tokio::net::TcpListener::bind(address).await?;
    println!("tackly sync listening on {address}");
    axum::serve(listener, tackly_sync::router(pool)).await?;
    Ok(())
}
