#![feature(async_await, await_macro, futures_api)]
#![recursion_limit = "128"]

use pokemon_showdown_client::message::{Chat, Kind, UpdateUser};
use pokemon_showdown_client::{connect, Result};
use std::env;
use tokio::await;

async fn start(login: String, password: String) -> Result<()> {
    let (mut sender, mut receiver) = await!(connect("showdown"))?;
    loop {
        let message = await!(receiver.receive())?;
        let parsed = message.parse();
        match parsed.kind {
            Kind::Challenge(ch) => await!(ch.login_with_password(&mut sender, &login, &password))?,
            Kind::UpdateUser(UpdateUser { named: true, .. }) => {
                await!(sender.send_global_command("join bot dev"))?;
            }
            Kind::Chat(Chat {
                user,
                message: ".yay",
                ..
            }) => {
                let response = format!("YAY {}!", user.to_uppercase());
                await!(sender.send_chat_message(parsed.room_id, &response))?
            }
            _ => {}
        }
    }
}

fn main() {
    let mut args = env::args().skip(1);
    let login = args.next().unwrap();
    let password = args.next().unwrap();
    tokio::run_async(async { await!(start(login, password)).unwrap() });
}