use std::time::Duration;
use usboot::amlogic::{AmlogicSoC, SocId};

#[tokio::test]
async fn socid() -> anyhow::Result<()> {
    let dev = AmlogicSoC::with_defaults(Duration::from_secs(5)).await?;

    let socid_str = dev.identify().await?;
    let socid = SocId::new(&socid_str);

    println!("Firmware Version {}", socid);

    Ok(())
}