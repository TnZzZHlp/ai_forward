use once_cell::sync::OnceCell;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sqlx::{query_as, Postgres};
use tracing::trace;

pub static DB: OnceCell<DatabaseClient> = OnceCell::new();

use crate::CACHE;
use crate::CONFIG;

#[derive(Debug, sqlx::FromRow, Serialize, Deserialize)]
pub struct AIRequest {
    pub id: i64,
    pub messages: Value,
    pub response: String,
}

#[derive(Debug)]
pub struct DatabaseClient {
    pub pool: sqlx::Pool<Postgres>,
}

impl DatabaseClient {
    pub async fn init<U>(url: U) -> Self
    where
        U: AsRef<str>,
    {
        let pool = sqlx::PgPool::connect(url.as_ref()).await.unwrap();
        DatabaseClient { pool }
    }

    pub async fn load_cache(&self) {
        let rows = query_as::<_, AIRequest>("SELECT * FROM ai_requests ORDER BY id DESC LIMIT $1")
            .bind(CONFIG.get().unwrap().cache_size as i32)
            .fetch_all(&self.pool)
            .await
            .unwrap();

        for row in rows {
            CACHE
                .get()
                .unwrap()
                .insert(row.messages, row.response.into());
        }
    }

    pub async fn save_to_db(&self, messages: &Value, response: &String) {
        match sqlx::query("INSERT INTO ai_requests (messages, response) VALUES ($1, $2)")
            .bind(messages)
            .bind(response)
            .execute(&self.pool)
            .await
        {
            Ok(_) => {
                // Successfully inserted
                trace!(
                    "Inserted into database: messages = {:?}, response = {:?}",
                    messages,
                    response
                );
            }
            Err(e) => {
                // Handle error
                trace!(
                    "Failed to insert into database: messages = {:?}, response = {:?}, error = {}",
                    messages,
                    response,
                    e
                );
            }
        };
    }

    pub async fn get_from_db(&self, messages: &Value) -> Option<AIRequest> {
        sqlx::query_as::<_, AIRequest>("SELECT * FROM ai_requests WHERE messages = $1")
            .bind(messages)
            .fetch_optional(&self.pool)
            .await
            .unwrap()
    }
}
