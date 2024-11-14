use std::{future::Future, pin::Pin};

use reth_chainspec::Hardforks;
use reth_provider::{BlockReader, ChainSpecProvider, HeaderProvider};

use crate::{
    testnet_controllers::{AngstromTestnet, StateMachineTestnet},
    types::StateMachineActionHookFn
};

pub trait WithAction<'a, C>
where
    C: BlockReader
        + HeaderProvider
        + ChainSpecProvider
        + Unpin
        + Clone
        + ChainSpecProvider<ChainSpec: Hardforks>
        + 'static
{
    type FunctionOutput = StateMachineActionHookFn<'a, C>;

    fn advance_block(&mut self);
}

impl<'a, C> WithAction<'a, C> for StateMachineTestnet<'a, C>
where
    C: BlockReader
        + HeaderProvider
        + ChainSpecProvider
        + Unpin
        + Clone
        + ChainSpecProvider<ChainSpec: Hardforks>
        + 'static
{
    fn advance_block(&mut self) {
        let f = |testnet: &'a mut AngstromTestnet<C>| pin_action(testnet.all_peers_update_state(0));
        self.add_action("advance block", f);
    }
}

fn pin_action<'a, F>(fut: F) -> Pin<Box<dyn Future<Output = eyre::Result<()>> + Send + 'a>>
where
    F: Future<Output = eyre::Result<()>> + Send + 'a
{
    Box::pin(fut)
}
