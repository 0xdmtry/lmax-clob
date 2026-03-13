use rand::Rng;
use yellowstone_grpc_proto::geyser::{
    subscribe_update::UpdateOneof, SubscribeUpdate, SubscribeUpdateTransaction,
    SubscribeUpdateTransactionInfo,
};

pub fn generate_update(_payload_size_bytes: usize) -> SubscribeUpdate {
    let mut rng = rand::thread_rng();

    let signature: Vec<u8> = (0..64).map(|_| rng.gen::<u8>()).collect();

    SubscribeUpdate {
        filters: vec!["orders".to_string()],
        created_at: None,
        update_oneof: Some(UpdateOneof::Transaction(SubscribeUpdateTransaction {
            transaction: Some(SubscribeUpdateTransactionInfo {
                signature,
                is_vote: false,
                transaction: None,
                meta: None,
                index: rng.gen::<u64>(),
            }),
            slot: rng.gen::<u64>(),
        })),
    }
}
