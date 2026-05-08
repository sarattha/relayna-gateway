use redis::aio::MultiplexedConnection;

#[derive(Clone)]
pub struct RedisReadiness {
    client: redis::Client,
}

impl RedisReadiness {
    pub fn new(redis_url: &str) -> redis::RedisResult<Self> {
        Ok(Self {
            client: redis::Client::open(redis_url)?,
        })
    }

    pub async fn ready(&self) -> redis::RedisResult<()> {
        let mut connection: MultiplexedConnection =
            self.client.get_multiplexed_async_connection().await?;
        redis::cmd("PING")
            .query_async::<String>(&mut connection)
            .await
            .map(|_| ())
    }
}
