// Live smoke: connect to WFM's order stream, print the first few orders, stop.
// Run by hand: cargo run -p wfm-core --example ws_smoke
fn main() -> anyhow::Result<()> {
    let start = std::time::Instant::now();
    let mut n = 0u32;
    let count = std::cell::Cell::new(0u32);
    let mut on = |o: wfm_core::ws::NewOrder| {
        n += 1;
        count.set(n);
        eprintln!("{} {} {}p item={} rank={:?} by={:?}", o.side, o.quantity, o.platinum, o.item_id, o.rank, o.user_name);
    };
    let stop = || start.elapsed().as_secs() > 15 || count.get() >= 8;
    wfm_core::ws::run_new_orders_stream("pc", &mut on, &stop)?;
    eprintln!("got {} orders in {:?}", count.get(), start.elapsed());
    Ok(())
}
