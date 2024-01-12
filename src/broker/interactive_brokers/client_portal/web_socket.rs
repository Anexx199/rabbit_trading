use futures_util::{
    stream::{SplitSink, SplitStream},
    SinkExt, StreamExt,
};
use serde_json::json;
use tokio::net::TcpStream;
use tokio_tungstenite::{tungstenite::Message, MaybeTlsStream, WebSocketStream};

use super::{client::IBClientPortal, model::web_socket::Subscription};
use crate::{
    model::common::error::Error, utils::error::tokio_tungstenite_error_to_rabbit_trading_error,
};

pub type WriteWs = SplitSink<WebSocketStream<MaybeTlsStream<TcpStream>>, Message>;
pub type ReadWs = SplitStream<WebSocketStream<MaybeTlsStream<TcpStream>>>;

//https://interactivebrokers.github.io/cpwebapi/websockets
pub async fn listen(reader: &mut ReadWs, on_message: fn(String) -> ()) -> Result<(), Error> {
    while let Some(msg) = reader.next().await {
        on_message(
            msg.map_err(tokio_tungstenite_error_to_rabbit_trading_error)?
                .into_text()
                .map_err(tokio_tungstenite_error_to_rabbit_trading_error)?,
        );
    }
    Ok(())
}

/// Send the required message every 58 seconds to keep the connection alive
/// https://interactivebrokers.github.io/cpwebapi/websockets#echo
pub async fn keep_alive(mut writer: WriteWs) -> Result<(), Error> {
    let mut interval = tokio::time::interval(tokio::time::Duration::from_secs(58));
    loop {
        interval.tick().await;
        writer
            .send(Message::Text("tic".to_owned()))
            .await
            .map_err(tokio_tungstenite_error_to_rabbit_trading_error)?;
    }
}

impl IBClientPortal {
    fn get_ws_url(&self) -> String {
        let protocol = if self.listen_ssl { "wss" } else { "ws" };
        format!("{protocol}://{}/v1/api/ws", self.host)
    }

    fn ws_auth_msg(&self, session: String) -> String {
        json!({ "session": session }).to_string()
    }

    pub async fn connect_to_websocket(
        &self,
        subscriptions: Vec<Subscription>,
        on_message: fn(String) -> (),
    ) -> Result<(), Error> {
        let url = self.get_ws_url();
        let (ws_stream, _) = tokio_tungstenite::connect_async(url)
            .await
            .map_err(tokio_tungstenite_error_to_rabbit_trading_error)?;
        let (mut ws_out, mut ws_in) = ws_stream.split();

        let session = self.tickle().await.unwrap().session;
        ws_out
            .send(Message::Text(self.ws_auth_msg(session).to_owned()))
            .await
            .map_err(tokio_tungstenite_error_to_rabbit_trading_error)?;

        //tokio::time::sleep(tokio::time::Duration::from_millis(300)).await;
        for sub in subscriptions {
            ws_out
                .send(Message::Text(sub.build()))
                .await
                .map_err(tokio_tungstenite_error_to_rabbit_trading_error)?;
        }
        tokio::try_join!(listen(&mut ws_in, on_message), keep_alive(ws_out))?;
        Ok(())
    }
}
