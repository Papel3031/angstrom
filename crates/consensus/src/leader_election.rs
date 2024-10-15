use std::{cmp::Ordering, collections::HashSet, default::Default};

use reth_rpc_types::PeerId;

#[derive(Clone, Debug)]
pub struct Validator {
    peer_id:      PeerId,
    voting_power: u64,
    priority:     f64
}

impl Validator {
    fn new(name: PeerId, voting_power: u64) -> Self {
        Validator { peer_id: name, voting_power, priority: 0.0 }
    }
}

pub struct WeightedRoundRobin {
    validators:                HashSet<Validator>,
    new_joiner_penalty_factor: f64
}

impl WeightedRoundRobin {
    pub fn new(validators: Vec<Validator>, new_joiner_penalty_factor: Option<f64>) -> Self {
        WeightedRoundRobin {
            validators:                HashSet::from_iter(validators),
            // apparently that's a good value https://docs.cometbft.com/v0.38/spec/consensus/proposer-selection#new-validator
            new_joiner_penalty_factor: new_joiner_penalty_factor.unwrap_or(1.125)
        }
    }

    fn proposer_selection(&mut self) -> PeerId {
        let total_voting_power: u64 = self.validators.iter().map(|v| v.voting_power).sum();

        let mut updated_validators = HashSet::new();
        for mut validator in self.validators.drain() {
            validator.priority += validator.voting_power as f64;
            updated_validators.insert(validator);
        }
        self.validators = updated_validators;

        let mut proposer = self
            .validators
            .iter()
            .max_by(|a, b| {
                a.priority
                    .partial_cmp(&b.priority)
                    .unwrap_or(Ordering::Equal)
            })
            .unwrap()
            .clone();
        proposer.priority -= total_voting_power as f64;
        let proposer_name = proposer.peer_id.clone();
        self.validators.replace(proposer);

        proposer_name
    }

    fn center_priorities(&mut self) {
        let avg_priority: f64 =
            self.validators.iter().map(|v| v.priority).sum::<f64>() / self.validators.len() as f64;
        let mut updated_validators = HashSet::new();
        for mut validator in self.validators.drain() {
            validator.priority -= avg_priority;
            updated_validators.insert(validator);
        }
        self.validators = updated_validators;
    }

    fn scale_priorities(&mut self) {
        let max_priority = self
            .validators
            .iter()
            .map(|v| v.priority)
            .fold(f64::NEG_INFINITY, f64::max);
        let min_priority = self
            .validators
            .iter()
            .map(|v| v.priority)
            .fold(f64::INFINITY, f64::min);
        let total_voting_power: u64 = self.validators.iter().map(|v| v.voting_power).sum();
        let diff = max_priority - min_priority;
        let threshold = 2.0 * total_voting_power as f64;

        if diff > threshold {
            let scale = diff / threshold;
            let mut updated_validators = HashSet::new();
            for mut validator in self.validators.drain() {
                validator.priority /= scale;
                updated_validators.insert(validator);
            }
            self.validators = updated_validators;
        }
    }

    pub fn choose_proposer(&mut self) -> PeerId {
        self.center_priorities();
        self.scale_priorities();
        self.proposer_selection()
    }

    fn remove_validator(&mut self, peer_id: &PeerId) {
        let validator = Validator::new(*peer_id, 0);
        self.validators.remove(&validator);
    }

    fn add_validator(&mut self, peer_id: PeerId, voting_power: u64) {
        let mut new_validator = Validator::new(peer_id, voting_power);
        let total_voting_power: u64 = self.validators.iter().map(|v| v.voting_power).sum();
        new_validator.priority -= self.new_joiner_penalty_factor * total_voting_power as f64;
        self.validators.insert(new_validator);
    }
}

impl PartialEq for Validator {
    fn eq(&self, other: &Self) -> bool {
        self.peer_id == other.peer_id
    }
}

impl Eq for Validator {}

impl std::hash::Hash for Validator {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.peer_id.hash(state);
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use super::*;

    #[test]
    fn test_round_robin_simulation() {
        let peers = HashMap::from([
            ("Alice".to_string(), PeerId::random()),
            ("Bob".to_string(), PeerId::random()),
            ("Charlie".to_string(), PeerId::random())
        ]);
        let validators = vec![
            Validator::new(peers["Alice"].clone(), 100),
            Validator::new(peers["Bob"].clone(), 200),
            Validator::new(peers["Charlie"].clone(), 300),
        ];
        let mut algo = WeightedRoundRobin::new(validators, None);

        fn simulate_rounds(algo: &mut WeightedRoundRobin, rounds: usize) -> HashMap<PeerId, usize> {
            let mut stats = HashMap::new();
            for _ in 0..rounds {
                let proposer = algo.choose_proposer();
                *stats.entry(proposer).or_insert(0) += 1;
            }
            stats
        }

        let rounds = 1000;
        let stats = simulate_rounds(&mut algo, rounds);

        assert_eq!(stats.len(), 3);

        let total_selections: usize = stats.values().sum();
        assert_eq!(total_selections, rounds);

        let alice_ratio = *stats.get(&peers["Alice"]).unwrap() as f64 / rounds as f64;
        let bob_ratio = *stats.get(&peers["Bob"]).unwrap() as f64 / rounds as f64;
        let charlie_ratio = *stats.get(&peers["Charlie"]).unwrap() as f64 / rounds as f64;

        assert!((alice_ratio - 0.167).abs() < 0.05);
        assert!((bob_ratio - 0.333).abs() < 0.05);
        assert!((charlie_ratio - 0.5).abs() < 0.05);
    }

    #[test]
    fn test_add_remove_validator() {
        let peers = HashMap::from([
            ("Alice".to_string(), PeerId::random()),
            ("Bob".to_string(), PeerId::random()),
            ("Charlie".to_string(), PeerId::random())
        ]);
        let validators = vec![
            Validator::new(peers["Alice"].clone(), 100),
            Validator::new(peers["Bob"].clone(), 200),
        ];
        let mut algo = WeightedRoundRobin::new(validators, None);

        fn simulate_rounds(algo: &mut WeightedRoundRobin, rounds: usize) -> HashMap<PeerId, usize> {
            let mut stats = HashMap::new();
            for _ in 0..rounds {
                let proposer = algo.choose_proposer();
                *stats.entry(proposer).or_insert(0) += 1;
            }
            stats
        }

        let rounds = 1000;
        let initial_stats = simulate_rounds(&mut algo, rounds);
        assert_eq!(initial_stats.len(), 2);

        algo.add_validator(peers["Charlie"].clone(), 300);

        let after_add_stats = simulate_rounds(&mut algo, rounds);
        assert_eq!(after_add_stats.len(), 3);
        assert!(after_add_stats.contains_key(&peers["Charlie"]));

        algo.remove_validator(&peers["Bob"]);

        let after_remove_stats = simulate_rounds(&mut algo, rounds);
        assert_eq!(after_remove_stats.len(), 2);
        assert!(!after_remove_stats.contains_key(&peers["Bob"]));
    }
}
