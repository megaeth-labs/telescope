use std::{
    io::{stdout, Write},
    time::Instant,
};

use chrono::Local;
use clap::Parser;

use alloy::{
    primitives::bytes::Buf,
    providers::{Provider, ProviderBuilder, WsConnect},
    rpc::types::{Block, BlockTransactionsKind},
};
use eyre::Result;
use futures_util::StreamExt;

/// A utility to monitor the MegaETH performance.
#[derive(Parser, Debug)]
#[command(version, about, long_about = None)]
struct Args {
    /// The WebSocket endpoint to connect to the blockchain.
    #[arg(short, long, default_value = "ws://localhost:8546")]
    endpoint: String,

    /// The window size (number of blocks) to measure the performance.
    #[arg(short, long, default_value = "16")]
    window: u64,

    /// Refresh the printed metrics.
    #[arg(short, long)]
    refresh: bool,
}

#[tokio::main]
async fn main() -> Result<()> {
    let args = Args::parse();
    assert!(args.window > 1, "Window size must be greater than 1");

    // Create the provider.
    let ws = WsConnect::new(args.endpoint);
    let provider = ProviderBuilder::new().on_ws(ws).await?;

    // Subscribe to new blocks.
    let sub = provider.subscribe_blocks().await?;
    let mut stream = sub.into_stream();

    // Create the measurement.
    let mut measurement = Measurement::new(args.window);

    while let Some(header) = stream.next().await {
        let block = provider
            .get_block_by_hash(header.hash, BlockTransactionsKind::Hashes)
            .await
            .expect("Failed to get block")
            .expect("Block does not exist");
        measurement.record(block);
        measurement.print(args.refresh);
    }

    Ok(())
}

struct Measurement {
    window_start: Instant,
    buffer: Vec<Datapoint>,
    window_size: u64,
}

impl Measurement {
    fn new(window_size: u64) -> Self {
        Self {
            window_start: Instant::now(),
            buffer: Vec::with_capacity(window_size as usize + 1),
            window_size,
        }
    }

    /// Get the size of the buffer.
    #[inline]
    #[allow(unused)]
    fn buffer_len(&self) -> usize {
        self.buffer.len()
    }

    /// Record a new block in the buffer.
    #[inline]
    fn record(&mut self, block: Block) {
        if let Some(last) = self.buffer.last() {
            if last.block.header.number >= block.header.number {
                return;
            }
        }
        self.buffer.push(Datapoint::new(block));
        if self.buffer.len() > self.window_size as usize {
            let data_point = self.buffer.remove(0);
            self.window_start = data_point.timestamp;
        }
    }

    /// Calculate the transactions per second (TPS) using the data in the buffer.
    #[inline]
    fn transactions_per_second(&self) -> f64 {
        let last_block = self.buffer.last().expect("Buffer is empty");
        let time_window = last_block.timestamp - self.window_start;
        let n_txs = self.buffer.iter().map(|b| b.transactions()).sum::<usize>();
        n_txs as f64 / time_window.as_secs_f64()
    }

    /// Calculate the gas per second (gas/s) using the data in the buffer.
    #[inline]
    fn gas_per_second(&self) -> f64 {
        let last_block = self.buffer.last().expect("Buffer is empty");
        let time_window = last_block.timestamp - self.window_start;
        let n_gas = self.buffer.iter().map(|b| b.gas_used()).sum::<u64>();
        n_gas as f64 / time_window.as_secs_f64()
    }

    /// Calculate the mini-block rate (mini-blocks/s) using the data in the buffer.
    #[inline]
    fn mini_block_rate(&self) -> f64 {
        let last_block = self.buffer.last().expect("Buffer is empty");
        let time_window = last_block.timestamp - self.window_start;
        let n_mini_blocks = self.buffer.iter().map(|b| b.mini_blocks()).sum::<u64>();
        n_mini_blocks as f64 / time_window.as_secs_f64()
    }

    /// Print the current measurements.
    #[inline]
    fn print(&self, refresh: bool) {
        let now = Local::now();
        print!(
            "\r[{}] Mini-block interval: {:.1} ms, TPS: {:.1}, Gas: {:.2} Mgas/s {}",
            now.format("%Y-%m-%d %H:%M:%S%.6f"),
            1000.0 / self.mini_block_rate(),
            self.transactions_per_second(),
            self.gas_per_second() / 1_000_000.0,
            if refresh { "" } else { "\n" }
        );
        stdout().flush().unwrap();
    }
}

/// Contains the data we sample from the blockchain.
struct Datapoint {
    timestamp: Instant,
    block: Block,
}

impl Datapoint {
    fn new(block: Block) -> Self {
        Self {
            timestamp: Instant::now(),
            block,
        }
    }

    /// Get the gas used by the block.
    #[inline]
    fn gas_used(&self) -> u64 {
        self.block.header.gas_used
    }

    /// Get the number of transactions in the block.
    #[inline]
    fn transactions(&self) -> usize {
        self.block.transactions.len()
    }

    /// Calculate the number of mini-blocks in the block.
    #[inline]
    fn mini_blocks(&self) -> u64 {
        let mut buf = self.block.header.extra_data.clone();
        let fragment_count = buf.get_u8();
        fragment_count as u64
    }
}
