use std::net::{SocketAddr, ToSocketAddrs};

use bitcoin::{consensus::Decodable, Transaction};
use demand_easy_sv2::const_sv2::MESSAGE_TYPE_REQUEST_TRANSACTION_DATA_SUCCESS;
use demand_easy_sv2::{
    const_sv2::MESSAGE_TYPE_NEW_TEMPLATE,
    roles_logic_sv2::{
        common_messages_sv2::Protocol,
        parsers::TemplateDistribution,
        template_distribution_sv2::{CoinbaseOutputDataSize, RequestTransactionData},
    },
    ClientBuilder, PoolMessages,
};
use tokio::net::TcpStream;

const TP_ADDRESS: &str = "127.0.0.1:8442";
const COINBASE_OUT_DATA_SIZE: u32 = 34;

#[tokio::main]
async fn main() {
    dotenv::dotenv().ok();
    let server = connect_to_server().await;
    let mut builder = ClientBuilder::new();
    builder
        .with_protocol(Protocol::TemplateDistributionProtocol)
        .expect("This can not fail since protocol is not yet set")
        .try_add_server(server)
        .await
        .expect("Impossible to add server to the ClientBuilder");
    add_new_template_handler(&mut builder);
    add_tx_data_handler(&mut builder);
    add_send_coinbase_out(&mut builder);
    let err = builder
        .try_build()
        .expect("Impossible to build the Client")
        .start()
        .await;
    eprintln!("{err:?}");
}

async fn connect_to_server() -> TcpStream {
    let server: SocketAddr = std::env::var("SERVER")
        .unwrap_or(TP_ADDRESS.to_string())
        .to_socket_addrs()
        .expect("Client address must be a valid address")
        .next()
        .unwrap();
    TcpStream::connect(server)
        .await
        .expect("Impossible to connect to server")
}

fn add_new_template_handler(builder: &mut ClientBuilder) {
    let (mut new_templates, send_up) = builder.add_handler_with_sender(MESSAGE_TYPE_NEW_TEMPLATE);
    tokio::spawn(async move {
        while let Some(PoolMessages::TemplateDistribution(TemplateDistribution::NewTemplate(m))) =
            new_templates.recv().await
        {
            let id = m.template_id;
            let value = m.coinbase_tx_value_remaining as f64 / 10_f64.powi(8);
            println!("Received new template with id {id} and value {value} btc");
            let m =
                PoolMessages::TemplateDistribution(TemplateDistribution::RequestTransactionData(
                    RequestTransactionData { template_id: id },
                ));
            if send_up.send(m).await.is_err() {
                eprintln!("Upstream is down");
                std::process::exit(1);
            }
        }
    });
}
fn add_tx_data_handler(builder: &mut ClientBuilder) {
    let mut new_templates = builder.add_handler(MESSAGE_TYPE_REQUEST_TRANSACTION_DATA_SUCCESS);
    tokio::spawn(async move {
        while let Some(PoolMessages::TemplateDistribution(
            TemplateDistribution::RequestTransactionDataSuccess(m),
        )) = new_templates.recv().await
        {
            let id = m.template_id;
            let mut total = 0;
            let txs: Vec<Vec<u8>> = m.transaction_list.to_vec();
            let len = txs.len();
            for tx in txs {
                let mut tx = &tx[..];
                let transaction = Transaction::consensus_decode_from_finite_reader(&mut tx)
                    .unwrap_or_else(|_| {
                        eprintln!("Impossible to decode transaction\n{tx:?}");
                        std::process::exit(1);
                    });
                total += transaction
                    .output
                    .iter()
                    .map(|o| o.value.to_sat())
                    .sum::<u64>();
            }
            let total = total as f64 / 10_f64.powi(8);
            println!(
                "Template with id {id} coinatains {len} transactions with total value {total} btc"
            );
        }
    });
}

fn add_send_coinbase_out(builder: &mut ClientBuilder) {
    let sender = builder.add_message_sender();
    let coinbase_out_size = std::env::var("COINBASE_OUT_DATA_SIZE")
        .unwrap_or(COINBASE_OUT_DATA_SIZE.to_string())
        .parse::<u32>()
        .expect("COINBASE_OUT_DATA_SIZE must be parsable in an u32");
    let coinbase_out_size = CoinbaseOutputDataSize {
        coinbase_output_max_additional_size: coinbase_out_size,
    };
    tokio::task::spawn(async move {
        sender
            .send(PoolMessages::TemplateDistribution(
                TemplateDistribution::CoinbaseOutputDataSize(coinbase_out_size),
            ))
            .await
            .expect("Impossible to send CoinbaseOutputDataSize");
        // We are not supposed to drop the sender the Client expect it to be around for the entire
        // life of the program
        loop {
            tokio::task::yield_now().await;
        }
    });
}
