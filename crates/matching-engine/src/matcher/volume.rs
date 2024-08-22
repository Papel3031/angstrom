use std::{borrow::Cow, cell::Cell, cmp::Ordering};

use alloy::primitives::U256;
use angstrom_types::{
    matching::{Ray, SqrtPriceX96},
    orders::{NetAmmOrder, OrderFillState, OrderOutcome, PoolSolution},
    sol_bindings::{
        grouped_orders::{GroupedVanillaOrder, OrderWithStorageData},
        sol::TopOfBlockOrder
    }
};

use super::Solution;
use crate::{
    book::{
        order::{OrderContainer, OrderDirection, OrderExclusion},
        xpool::XPoolOutcomes,
        OrderBook
    },
    cfmm::uniswap::MarketPrice
};

type CrossPoolExclusions = Option<(Vec<Option<OrderExclusion>>, Vec<Option<OrderExclusion>>)>;

#[derive(Clone)]
pub struct VolumeFillMatcher<'a> {
    book:             &'a OrderBook,
    bid_idx:          Cell<usize>,
    pub bid_outcomes: Vec<OrderFillState>,
    bid_xpool:        Vec<Option<OrderExclusion>>,
    ask_idx:          Cell<usize>,
    pub ask_outcomes: Vec<OrderFillState>,
    ask_xpool:        Vec<Option<OrderExclusion>>,
    amm_price:        Option<MarketPrice<'a>>,
    amm_outcome:      Option<NetAmmOrder>,
    current_partial:  Option<OrderWithStorageData<GroupedVanillaOrder>>,
    results:          Solution,
    // A checkpoint should never have a checkpoint stored within itself, otherwise this gets gnarly
    checkpoint:       Option<Box<Self>>
}

impl<'a> VolumeFillMatcher<'a> {
    pub fn new(book: &'a OrderBook) -> Self {
        let mut new_element = Self::with_related(book, None);
        // We can checkpoint our initial state as valid
        new_element.save_checkpoint();
        new_element
    }

    fn with_related(book: &'a OrderBook, xpool: CrossPoolExclusions) -> Self {
        let (bid_xpool, ask_xpool) =
            xpool.unwrap_or_else(|| (vec![None; book.bids().len()], vec![None; book.asks().len()]));
        let bid_outcomes = vec![OrderFillState::Unfilled; book.bids().len()];
        let ask_outcomes = vec![OrderFillState::Unfilled; book.asks().len()];
        let amm_price = book.amm().map(|a| a.current_position());
        Self {
            book,
            bid_idx: Cell::new(0),
            bid_outcomes,
            bid_xpool,
            ask_idx: Cell::new(0),
            ask_outcomes,
            ask_xpool,
            amm_price,
            amm_outcome: None,
            current_partial: None,
            results: Solution::default(),
            checkpoint: None
        }
    }

    pub fn results(&self) -> &Solution {
        &self.results
    }

    /// Gets the relevant outcomes for cross-pool orders that have components
    /// that are part of this pool.  Returns an Option - `None` implies that
    /// there are no cross-pool orders in this pool.  Otherwise we get an
    /// XPoolOutcomes struct
    // pub fn crosspool_outcomes(&self) -> Option<XPoolOutcomes> {
    //     let mut live: Vec<OrderCoordinate> = Vec::new();
    //     let mut dead: Vec<OrderCoordinate> = Vec::new();
    //     let related_bids = self
    //         .book
    //         .bids()
    //         .iter()
    //         .enumerate()
    //         .filter(|(_, o)| o.related().is_some())
    //         .map(|(i, o)| (OrderDirection::Bid, i, o));
    //     let related_asks = self
    //         .book
    //         .asks()
    //         .iter()
    //         .enumerate()
    //         .filter(|(_, o)| o.related().is_some())
    //         .map(|(i, o)| (OrderDirection::Ask, i, o));
    //     // For each order that's part of a related order group
    //     for (direction, index, order) in related_bids.chain(related_asks) {
    //         // Figure out what it's outcome was
    //         let outcomes = match direction {
    //             OrderDirection::Bid => &self.bid_outcomes,
    //             OrderDirection::Ask => &self.ask_outcomes
    //         };
    //         if outcomes[index].is_filled() {
    //             // In our current configuration, the order was filled and should
    //             // be live
    //             live.extend(order.related().unwrap().iter().cloned()); // Can unwrap because we
    // checked earlier         } else {
    //             // The order was not filled and should be dead
    //             dead.extend(order.related().unwrap().iter().cloned()); // Can unwrap because we
    // checked earlier         }
    //     }
    //     if live.is_empty() && dead.is_empty() {
    //         // In the rare situation that it's all empty, we can just say none
    //         None
    //     } else {
    //         Some(XPoolOutcomes::new(live, dead))
    //     }
    // }

    #[allow(dead_code)]
    fn update_xpool(&self, state: &XPoolOutcomes) -> Option<Self> {
        let mut new_bid_xpool = Cow::from(&self.bid_xpool);
        let mut new_ask_xpool = Cow::from(&self.ask_xpool);

        let local_outcomes = state.for_book(self.book.id());

        // Vivify our live orders if they're dead
        for coord in local_outcomes.live().iter() {
            let (direction, idx) = if let Some((d, i)) = self.book.find_coordinate(coord) {
                (d, i)
            } else {
                continue;
            };
            let pool = match direction {
                OrderDirection::Bid => &mut new_bid_xpool,
                OrderDirection::Ask => &mut new_ask_xpool
            };
            if let Some(exc @ OrderExclusion::Dead(_)) = &pool[idx] {
                let new_state = exc.flip();
                // TTL limit for how many times we'll flip something away from Dead
                if new_state.ttl() < 6 {
                    pool.to_mut()[idx].replace(new_state);
                }
            }
        }

        // Kill our dead orders if they're alive
        for coord in local_outcomes.dead().iter() {
            let (direction, idx) = if let Some((d, i)) = self.book.find_coordinate(coord) {
                (d, i)
            } else {
                continue;
            };
            let pool = match direction {
                OrderDirection::Bid => &mut new_bid_xpool,
                OrderDirection::Ask => &mut new_ask_xpool
            };
            match &pool[idx] {
                None => {
                    pool.to_mut()[idx] = Some(OrderExclusion::Dead(0));
                }
                Some(exc @ OrderExclusion::Live(_)) => {
                    let new_state = exc.flip();
                    if new_state.ttl() < 6 {
                        pool.to_mut()[idx].replace(new_state);
                    }
                }
                _ => ()
            };
        }

        // If we've changed anything, we spawn a new state to be resolved
        if let (Cow::Owned(_), _) | (_, Cow::Owned(_)) = (&new_bid_xpool, &new_ask_xpool) {
            // Time to clone and return something new
            Some(Self::with_related(
                self.book,
                Some((new_bid_xpool.into_owned(), new_ask_xpool.into_owned()))
            ))
        } else {
            // Nothing was changed, we have no update
            None
        }
    }

    /// Save our current solve state to an internal checkpoint
    fn save_checkpoint(&mut self) {
        let checkpoint = Self {
            book:            self.book,
            bid_idx:         self.bid_idx.clone(),
            bid_outcomes:    self.bid_outcomes.clone(),
            bid_xpool:       self.bid_xpool.clone(),
            ask_idx:         self.ask_idx.clone(),
            ask_outcomes:    self.ask_outcomes.clone(),
            ask_xpool:       self.ask_xpool.clone(),
            amm_price:       self.amm_price.clone(),
            amm_outcome:     self.amm_outcome.clone(),
            current_partial: self.current_partial.clone(),
            results:         self.results.clone(),
            checkpoint:      None
        };
        self.checkpoint = Some(Box::new(checkpoint));
    }

    /// Spawn a new VolumeFillBookSolver from our checkpoint
    pub fn from_checkpoint(&self) -> Option<Self> {
        self.checkpoint.as_ref().map(|cp| *cp.clone())
    }

    /// Restore our checkpoint into this VolumeFillBookSolver - not sure if we
    /// ever want to do this but we can!
    #[allow(dead_code)]
    fn restore_checkpoint(&mut self) -> bool {
        let Some(checkpoint) = self.checkpoint.take() else {
            return false;
        };
        let Self {
            bid_idx, bid_outcomes, ask_idx, ask_outcomes, amm_price, current_partial, ..
        } = *checkpoint;
        self.bid_idx = bid_idx;
        self.bid_outcomes = bid_outcomes;
        self.ask_idx = ask_idx;
        self.ask_outcomes = ask_outcomes;
        self.amm_price = amm_price;
        self.current_partial = current_partial;
        true
    }

    pub fn fill(&mut self) {
        {
            // Local temporary storage for bid and ask AMM orders since they're generated on
            // the fly and need somewhere to live while we use them
            let mut amm_bid_order: Option<OrderContainer>;
            let mut amm_ask_order: Option<OrderContainer>;
            loop {
                let bid = match self.current_partial {
                    Some(ref o) if o.is_bid => OrderContainer::Partial(o),
                    _ => {
                        amm_bid_order = self.try_next_order(OrderDirection::Bid);
                        let Some(o) = amm_bid_order.or_else(|| {
                            self.book
                                .bids()
                                .get(self.bid_idx.get())
                                .map(OrderContainer::BookOrder)
                        }) else {
                            break;
                            // Break if there are no more valid bid orders to
                            // work with
                        };
                        o
                    }
                };
                let ask = match self.current_partial {
                    Some(ref o) if !o.is_bid => OrderContainer::Partial(o),
                    _ => {
                        amm_ask_order = self.try_next_order(OrderDirection::Ask);
                        let Some(o) = amm_ask_order.or_else(|| {
                            self.book
                                .asks()
                                .get(self.ask_idx.get())
                                .map(OrderContainer::BookOrder)
                        }) else {
                            break;
                            // Break if there are no more valid bid orders to
                            // work with
                        };
                        o
                    }
                };

                // If we're talking to the AMM on both sides, we're done
                if bid.is_amm() && ask.is_amm() {
                    break;
                }

                // If our prices no longer cross, we're done
                if ask.price() > bid.price() {
                    break;
                }

                // Limit to price so that AMM orders will only offer the quantity they can
                // profitably sell.  (Non-AMM orders ignore the provided price)
                let ask_q = ask.quantity(bid.price());
                let bid_q = bid.quantity(ask.price());

                // If either quantity is zero maybe we should break here? (could be a
                // replacement for price cross checking if we implement that)
                if ask_q == U256::ZERO || bid_q == U256::ZERO {
                    break;
                }

                let matched = ask_q.min(bid_q);
                let excess = bid_q - ask_q;
                // Store the amount we matched
                self.results.total_volume += matched;

                // Record partial fills
                if bid.is_partial() {
                    self.results.partial_volume.0 += matched;
                }
                if ask.is_partial() {
                    self.results.partial_volume.1 += matched;
                }

                // If bid or ask was an AMM order, we update our AMM stats
                if let (OrderContainer::AMM(o), _) | (_, OrderContainer::AMM(o)) = (&bid, &ask) {
                    // We always update our AMM price with any quantity sold
                    let final_amm_order = o.fill(matched);
                    self.amm_price = Some(final_amm_order.end_bound.clone());
                    // Add to our solution
                    self.results.amm_volume += matched;
                    self.results.amm_final_price = Some(*final_amm_order.end_bound.price());
                    // Update our overall AMM volume
                    let amm_out = self
                        .amm_outcome
                        .get_or_insert_with(|| NetAmmOrder::new(bid.is_amm()));
                    amm_out.add_quantity(matched);
                }

                // Then we see what else we need to do
                match excess.cmp(&U256::ZERO) {
                    Ordering::Equal => {
                        // We annihilated
                        self.results.price =
                            Some((*(ask.price() + bid.price()) / U256::from(2)).into());
                        // self.results.price = Some((ask.price() + bid.price()) / 2.0_f64);
                        // Mark as filled if non-AMM order
                        if !ask.is_amm() {
                            self.ask_outcomes[self.ask_idx.get()] = OrderFillState::CompleteFill
                        }
                        if !bid.is_amm() {
                            self.bid_outcomes[self.bid_idx.get()] = OrderFillState::CompleteFill
                        }
                        self.current_partial = None;
                        // Take a snapshot as a good solve state
                        self.save_checkpoint();
                        // We're done here, we'll get our next bid and ask on
                        // the next round
                    }
                    Ordering::Greater => {
                        self.results.price = Some(bid.price());
                        // Ask was completely filled, remainder bid
                        if !ask.is_amm() {
                            self.ask_outcomes[self.ask_idx.get()] = OrderFillState::CompleteFill
                        }
                        // Create and save our partial bid
                        if !bid.is_amm() {
                            self.bid_outcomes[self.bid_idx.get()] =
                                self.bid_outcomes[self.bid_idx.get()].partial_fill(matched);
                            self.current_partial = Some(bid.fill(ask_q));
                        } else {
                            self.current_partial = None;
                        }
                    }
                    Ordering::Less => {
                        self.results.price = Some(ask.price());
                        // Bid was completely filled, remainder ask
                        if !bid.is_amm() {
                            self.bid_outcomes[self.bid_idx.get()] = OrderFillState::CompleteFill
                        }
                        // Create and save our parital ask
                        if !ask.is_amm() {
                            self.ask_outcomes[self.ask_idx.get()] =
                                self.ask_outcomes[self.ask_idx.get()].partial_fill(matched);
                            self.current_partial = Some(ask.fill(bid_q));
                        } else {
                            self.current_partial = None;
                        }
                    }
                }
                // We can checkpoint if we annihilated (No partial), if we completely filled an
                // order with an AMM order (No partial) or if we have an incomplete order but
                // it's a Partial Fill order which means this is a valid state to stop
                let good_state = self
                    .current_partial
                    .as_ref()
                    .map(|o| matches!(o.order, GroupedVanillaOrder::Partial(_)))
                    .unwrap_or(true); // None is a good state
                if good_state {
                    self.save_checkpoint();
                }
            }
        }
    }

    fn try_next_order<'b>(&self, direction: OrderDirection) -> Option<OrderContainer<'a, 'b>> {
        let (index_cell, order_book, outcomes, related) = match direction {
            OrderDirection::Bid => {
                (&self.bid_idx, self.book.bids(), &self.bid_outcomes, &self.bid_xpool)
            }
            OrderDirection::Ask => {
                (&self.ask_idx, self.book.asks(), &self.ask_outcomes, &self.ask_xpool)
            }
        };
        let mut index = index_cell.get();
        // Find our next unfilled order
        while index < outcomes.len() {
            match (&outcomes[index], &related[index]) {
                // Always skip explicitly dead orders
                (_, Some(OrderExclusion::Dead(_))) => index += 1,
                // Otherwise, stop at the first Unfilled order
                (OrderFillState::Unfilled, _) => break,
                // Any other outcome gets skipped
                _ => index += 1
            }
        }

        // See if we have an AMM order that takes precedence over our book order
        let amm_order = self
            .amm_price
            .as_ref()
            .and_then(|amm_price| {
                let target_price = order_book
                    .get(index)
                    // .map(|o| SqrtPriceX96::from(OrderContainer::BookOrder(o).price()));
                    .map(|o| SqrtPriceX96::from(Ray::from(*OrderContainer::BookOrder(o).price())));
                amm_price.order_to_target(target_price, direction.is_ask())
            })
            .map(OrderContainer::AMM);

        // If we have no AMM order to look at, point the index at our next order
        if amm_order.is_none() {
            index_cell.set(index);
        }
        amm_order
    }

    pub fn solution(
        &self,
        searcher: Option<OrderWithStorageData<TopOfBlockOrder>>
    ) -> PoolSolution {
        let limit = self
            .bid_outcomes
            .iter()
            .enumerate()
            .map(|(idx, outcome)| (self.book.bids()[idx].order_id, outcome))
            .chain(
                self.ask_outcomes
                    .iter()
                    .enumerate()
                    .map(|(idx, outcome)| (self.book.asks()[idx].order_id, outcome))
            )
            .map(|(id, outcome)| OrderOutcome { id, outcome: outcome.clone() })
            .collect();
        PoolSolution { id: self.book.id(), amm_quantity: self.amm_outcome.clone(), searcher, limit }
    }
}
