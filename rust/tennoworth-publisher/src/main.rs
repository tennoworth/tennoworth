use std::{collections::BTreeSet, path::PathBuf};
use tennoworth_publisher::{
    auth::{Access, Auth},
    store::Store,
    App,
};

fn required(name: &str) -> anyhow::Result<String> {
    std::env::var(name).map_err(|_| anyhow::anyhow!("missing {name}"))
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let state = PathBuf::from(required("TENNOWORTH_PUBLISHER_STATE")?);
    let mut args = std::env::args().skip(1);
    match args.next().as_deref() {
        Some("backup") => {
            let destination = PathBuf::from(
                args.next()
                    .ok_or_else(|| anyhow::anyhow!("backup requires a new destination file"))?,
            );
            if args.next().is_some() {
                anyhow::bail!("unexpected backup arguments");
            }
            return tennoworth_publisher::store::backup_file(
                &state.join("announcements.sqlite"),
                &destination,
            );
        }
        Some("recover") => {
            if args.next().as_deref() != Some("--republish-restored-state") || args.next().is_some()
            {
                anyhow::bail!("recover requires --republish-restored-state");
            }
            return tennoworth_publisher::store::recover_publication(
                &state.join("announcements.sqlite"),
                &PathBuf::from(required("TENNOWORTH_PUBLISHER_FEED")?),
            );
        }
        Some(_) => anyhow::bail!("expected backup, recover or no command"),
        None => {}
    }
    let export = PathBuf::from(required("TENNOWORTH_PUBLISHER_FEED")?);
    let database = state.join("announcements.sqlite");
    let auth = Access::new(
        required("TENNOWORTH_ACCESS_ISSUER")?,
        required("TENNOWORTH_ACCESS_AUDIENCE")?,
        required("TENNOWORTH_PUBLISHERS")?
            .split(',')
            .map(|s| s.trim().to_owned())
            .collect::<BTreeSet<_>>(),
    )?;
    // The seed only creates first state, so a later loss of that file cannot stop the service.
    let store = if database.exists() {
        Store::resume(&database, export)?
    } else {
        let seed = std::fs::read_to_string(required("TENNOWORTH_PUBLISHER_SEED")?)?;
        Store::open(&database, export, &seed)?
    };
    let app = App::new(
        store,
        Auth::Access(auth),
        required("TENNOWORTH_PUBLISHER_ORIGIN")?,
    )?;
    let port = std::env::var("TENNOWORTH_PUBLISHER_PORT")
        .unwrap_or_else(|_| "8084".into())
        .parse()?;
    tennoworth_publisher::serve(app, port).await
}
