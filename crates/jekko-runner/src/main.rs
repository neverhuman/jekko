mod bin_main;

#[tokio::main(flavor = "multi_thread")]
async fn main() {
    bin_main::run().await;
}
