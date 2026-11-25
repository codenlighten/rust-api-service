//! Small closed-loop load generator for the notes API.
//!
//! Registers a fresh user, seeds notes, then runs `--concurrency` workers that each send one
//! request at a time over keep-alive connections for `--duration-secs`. Prints one result line.
//!
//! Closed loop means latency and throughput are coupled: a slow server receives fewer
//! requests, so the percentiles understate what an open-loop arrival process would see.

use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use anyhow::{bail, Context};
use reqwest::Client;
use serde_json::json;

struct Args {
    url: String,
    concurrency: usize,
    duration: Duration,
    warmup: Duration,
    write_percent: u64,
    seed_notes: usize,
}

fn parse_args() -> anyhow::Result<Args> {
    let mut args = Args {
        url: "http://127.0.0.1:3000".into(),
        concurrency: 32,
        duration: Duration::from_secs(10),
        warmup: Duration::from_secs(2),
        write_percent: 0,
        seed_notes: 200,
    };
    let mut it = std::env::args().skip(1);
    while let Some(flag) = it.next() {
        let mut value = || it.next().with_context(|| format!("{flag} needs a value"));
        match flag.as_str() {
            "--url" => args.url = value()?,
            "--concurrency" => args.concurrency = value()?.parse()?,
            "--duration-secs" => args.duration = Duration::from_secs(value()?.parse()?),
            "--warmup-secs" => args.warmup = Duration::from_secs(value()?.parse()?),
            "--write-percent" => args.write_percent = value()?.parse()?,
            "--seed-notes" => args.seed_notes = value()?.parse()?,
            other => bail!(
                "unknown flag '{other}'; flags: --url --concurrency --duration-secs --warmup-secs --write-percent --seed-notes"
            ),
        }
    }
    if args.concurrency == 0 || args.write_percent > 100 {
        bail!("--concurrency must be at least 1 and --write-percent at most 100");
    }
    Ok(args)
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let args = parse_args()?;
    let client = Client::builder()
        .pool_max_idle_per_host(args.concurrency)
        .build()?;

    let nanos = SystemTime::now().duration_since(UNIX_EPOCH)?.as_nanos();
    let creds =
        json!({ "email": format!("load-{nanos}@example.com"), "password": "load-test-password" });
    client
        .post(format!("{}/auth/register", args.url))
        .json(&creds)
        .send()
        .await
        .context("could not reach the server; is it running?")?
        .error_for_status()?;
    let session: serde_json::Value = client
        .post(format!("{}/auth/login", args.url))
        .json(&creds)
        .send()
        .await?
        .error_for_status()?
        .json()
        .await?;
    let token = session["access_token"]
        .as_str()
        .context("no access_token in login response")?
        .to_string();

    for i in 0..args.seed_notes {
        client
            .post(format!("{}/notes", args.url))
            .bearer_auth(&token)
            .json(&json!({ "title": format!("seed {i}"), "body": "x".repeat(200) }))
            .send()
            .await?
            .error_for_status()?;
    }

    let start = Instant::now();
    let measure_from = start + args.warmup;
    let end = measure_from + args.duration;

    let mut workers = Vec::new();
    for worker in 0..args.concurrency {
        let client = client.clone();
        let token = token.clone();
        let url = args.url.clone();
        let write_percent = args.write_percent;
        workers.push(tokio::spawn(async move {
            let mut latencies_us: Vec<u32> = Vec::new();
            let mut errors = 0u64;
            let mut n = worker as u64;
            while Instant::now() < end {
                n += 1;
                let write = (n * 37) % 100 < write_percent;
                let began = Instant::now();
                let request = if write {
                    client
                        .post(format!("{url}/notes"))
                        .json(&json!({ "title": "load", "body": "y".repeat(200) }))
                } else {
                    client.get(format!("{url}/notes?limit=20"))
                };
                let ok = match request.bearer_auth(&token).send().await {
                    Ok(resp) => {
                        let ok = resp.status().is_success();
                        resp.bytes().await.is_ok() && ok
                    }
                    Err(_) => false,
                };
                if began >= measure_from {
                    if ok {
                        latencies_us.push(began.elapsed().as_micros().min(u32::MAX as u128) as u32);
                    } else {
                        errors += 1;
                    }
                }
            }
            (latencies_us, errors)
        }));
    }

    let mut all: Vec<u32> = Vec::new();
    let mut errors = 0;
    for w in workers {
        let (l, e) = w.await?;
        all.extend(l);
        errors += e;
    }
    all.sort_unstable();
    if all.is_empty() {
        bail!("no successful requests were measured ({errors} errors)");
    }
    let pct = |p: f64| {
        all[((all.len() as f64 * p).ceil() as usize)
            .saturating_sub(1)
            .min(all.len() - 1)] as f64
            / 1000.0
    };
    println!(
        "requests={} errors={} rps={:.0} p50_ms={:.2} p99_ms={:.2} max_ms={:.2} concurrency={} write_percent={}",
        all.len(),
        errors,
        all.len() as f64 / args.duration.as_secs_f64(),
        pct(0.50),
        pct(0.99),
        *all.last().unwrap() as f64 / 1000.0,
        args.concurrency,
        args.write_percent,
    );
    Ok(())
}
