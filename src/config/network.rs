use lazy_static::lazy_static;
//pub const DEFAULT_RPC_PORT: &'static str = "8080";
lazy_static! {
    pub static ref PEERS: Vec<&'static str> = vec!["127.0.0.1:8080", "127.0.0.1:8081"];
}
