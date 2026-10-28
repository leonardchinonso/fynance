//! Share transfers between accounts, and between people.
//!
//! A Section 104 pool belongs to a person, so the engine pools each profile's
//! shares separately. A transfer between two accounts of one person changes
//! nothing: the shares and their cost stay in that person's pool. A transfer
//! to someone else is a disposal by the sender and an acquisition by the
//! receiver, at no gain and no loss between spouses (TCGA 1992 s58), and
//! otherwise as a gift at market value (s17).
//!
//! The ledger records each side of a transfer as its own `transfer` event (a
//! negative quantity leaving one account, a positive one arriving in another)
//! with no link between them, so they are paired here by symbol, size and date.

use std::collections::{HashMap, HashSet};

use chrono::Duration;
use rust_decimal::Decimal;

use crate::model::{Account, InvestmentEvent, InvestmentEventType, Profile};

/// The most days a transfer may take to arrive and still be paired with its departure.
pub(crate) const TRANSFER_PAIRING_DAYS: i64 = 7;

/// Who owns each account, and who is married to whom.
#[derive(Debug, Default)]
pub(crate) struct Ownership {
    /// The owning profile of every account that has exactly one.
    account_owner: HashMap<String, String>,
    spouse_of: HashMap<String, String>,
}

impl Ownership {
    pub(crate) fn new(accounts: &[Account], profiles: &[Profile]) -> Self {
        Ownership {
            account_owner: accounts
                .iter()
                .filter_map(|a| match a.profile_ids.as_slice() {
                    [only] => Some((a.id.clone(), only.clone())),
                    _ => None,
                })
                .collect(),
            spouse_of: profiles
                .iter()
                .filter_map(|p| Some((p.id.clone(), p.spouse_id.clone()?)))
                .collect(),
        }
    }

    pub(crate) fn owner(&self, account_id: &str) -> Option<&str> {
        self.account_owner.get(account_id).map(String::as_str)
    }

    fn are_spouses(&self, a: &str, b: &str) -> bool {
        self.spouse_of.get(a).is_some_and(|s| s == b)
    }
}

/// How a transfer of shares from one person to another is taxed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum TransferRule {
    /// Between spouses: the receiver takes over the sender's cost (TCGA s58).
    NoGainNoLoss,
    /// Between anyone else: a disposal and an acquisition at market value.
    MarketValue,
}

/// One side of a transfer of shares from one person to another.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct PersonTransfer {
    /// True on the departing side, false on the arriving side.
    pub(crate) outgoing: bool,
    /// The event id of the other side.
    pub(crate) counterpart_id: String,
    pub(crate) from: String,
    pub(crate) to: String,
    pub(crate) rule: TransferRule,
}

/// Pair each departing `transfer` (negative quantity) with an arriving one of
/// the same symbol and size, no earlier and at most [`TRANSFER_PAIRING_DAYS`]
/// later, preferring the closest. Moves between one person's own accounts are
/// paired first, so they are never mistaken for a transfer to someone else.
///
/// Returns both sides of every person-to-person transfer, keyed by event id.
/// Transfers within one person, and any side left unpaired, are absent: the
/// engine treats them as moving nothing.
pub(crate) fn pair_person_transfers(
    events: &[InvestmentEvent],
    ownership: &Ownership,
) -> HashMap<String, PersonTransfer> {
    let owned = |sign: fn(&Decimal) -> bool| -> Vec<(&InvestmentEvent, &str)> {
        let mut side: Vec<_> = events
            .iter()
            .filter(|e| e.event_type == InvestmentEventType::Transfer && sign(&e.quantity))
            .filter_map(|e| Some((e, ownership.owner(&e.account_id)?)))
            .collect();
        side.sort_by_key(|(e, _)| e.date);
        side
    };
    let outs = owned(Decimal::is_sign_negative);
    let ins = owned(|q| q.is_sign_positive() && !q.is_zero());

    let mut used_out = vec![false; outs.len()];
    let mut used_in = vec![false; ins.len()];
    let mut transfers = HashMap::new();
    for same_person in [true, false] {
        for (i, (arrival, receiver)) in ins.iter().enumerate() {
            if used_in[i] {
                continue;
            }
            let departure = outs
                .iter()
                .enumerate()
                .filter(|(o, (out, sender))| {
                    !used_out[*o]
                        && (sender == receiver) == same_person
                        && out.symbol == arrival.symbol
                        && -out.quantity == arrival.quantity
                        && out.date <= arrival.date
                        && arrival.date - out.date <= Duration::days(TRANSFER_PAIRING_DAYS)
                })
                .max_by_key(|(_, (out, _))| out.date);
            let Some((o, (out, sender))) = departure else {
                continue;
            };
            used_out[o] = true;
            used_in[i] = true;
            if same_person {
                continue;
            }
            let rule = if ownership.are_spouses(sender, receiver) {
                TransferRule::NoGainNoLoss
            } else {
                TransferRule::MarketValue
            };
            let leg = |outgoing: bool, counterpart: &InvestmentEvent| PersonTransfer {
                outgoing,
                counterpart_id: counterpart.id.clone(),
                from: sender.to_string(),
                to: receiver.to_string(),
                rule,
            };
            transfers.insert(out.id.clone(), leg(true, arrival));
            transfers.insert(arrival.id.clone(), leg(false, out));
        }
    }
    transfers
}

/// The profiles a report on `owners` depends on: those, plus everyone who
/// transferred shares to one of them, transitively, since a receiver's cost
/// can come from the sender's pool.
pub(crate) fn owners_needed(
    owners: &HashSet<String>,
    transfers: &HashMap<String, PersonTransfer>,
) -> HashSet<String> {
    let mut needed = owners.clone();
    loop {
        let senders: Vec<String> = transfers
            .values()
            .filter(|t| needed.contains(&t.to) && !needed.contains(&t.from))
            .map(|t| t.from.clone())
            .collect();
        if senders.is_empty() {
            return needed;
        }
        needed.extend(senders);
    }
}

/// The events a CGT report runs over, and what it shows of them.
#[derive(Debug)]
pub(crate) struct Ledger {
    pub(crate) events: Vec<InvestmentEvent>,
    pub(crate) ownership: Ownership,
    pub(crate) transfers: HashMap<String, PersonTransfer>,
    /// Accounts whose disposals the report emits. `None` means all.
    pub(crate) scope: Option<HashSet<String>>,
}

impl Ledger {
    /// Narrow `events` to what a report on the `scope` accounts (all when
    /// `None`) needs: the events of every profile owning a scoped account, and
    /// of everyone who transferred shares to them. A scoped account with
    /// several owners keeps its events, for `check_single_owner_accounts` to
    /// refuse. `excluded_accounts` (ISAs, pensions) never take part.
    pub(crate) fn build(
        events: Vec<InvestmentEvent>,
        ownership: Ownership,
        scope: Option<HashSet<String>>,
        excluded_accounts: &HashSet<String>,
    ) -> Self {
        let events: Vec<InvestmentEvent> = events
            .into_iter()
            .filter(|e| !excluded_accounts.contains(&e.account_id))
            .collect();
        let transfers = pair_person_transfers(&events, &ownership);
        let events = match &scope {
            None => events,
            Some(accounts) => {
                let scoped_owners: HashSet<String> = accounts
                    .iter()
                    .filter_map(|a| ownership.owner(a))
                    .map(str::to_string)
                    .collect();
                let needed = owners_needed(&scoped_owners, &transfers);
                events
                    .into_iter()
                    .filter(|e| match ownership.owner(&e.account_id) {
                        Some(owner) => needed.contains(owner),
                        None => accounts.contains(&e.account_id),
                    })
                    .collect()
            }
        };
        Ledger {
            events,
            ownership,
            transfers,
            scope,
        }
    }

    pub(crate) fn in_scope(&self, account_id: &str) -> bool {
        self.scope.as_ref().is_none_or(|s| s.contains(account_id))
    }

    /// Whether `owner`'s pools belong in the report.
    pub(crate) fn owner_in_scope(&self, owner: &str) -> bool {
        match &self.scope {
            None => true,
            Some(accounts) => accounts
                .iter()
                .any(|a| self.ownership.owner(a).unwrap_or(a.as_str()) == owner),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::NaiveDateTime;

    fn account(id: &str, owners: &[&str]) -> Account {
        Account {
            id: id.to_string(),
            name: id.to_string(),
            institution: String::new(),
            account_type: crate::model::AccountType::Investment,
            currency: "USD".to_string(),
            balance: None,
            balance_date: None,
            is_active: true,
            notes: None,
            profile_ids: owners.iter().map(|s| s.to_string()).collect(),
            is_stale: None,
            is_available: true,
        }
    }

    fn profile(id: &str, spouse: Option<&str>) -> Profile {
        Profile {
            id: id.to_string(),
            name: id.to_string(),
            utr: None,
            spouse_id: spouse.map(str::to_string),
        }
    }

    fn transfer(id: &str, account: &str, date: &str, quantity: i64) -> InvestmentEvent {
        InvestmentEvent {
            id: id.to_string(),
            account_id: account.to_string(),
            event_type: InvestmentEventType::Transfer,
            symbol: "PLTR".to_string(),
            date: NaiveDateTime::parse_from_str(&format!("{date}T00:00:00"), "%Y-%m-%dT%H:%M:%S")
                .unwrap(),
            quantity: Decimal::from(quantity),
            price_per_share: Decimal::from(150),
            fee: None,
            currency: "USD".to_string(),
            fee_currency: None,
            notes: None,
            fingerprint: id.to_string(),
            created_at: NaiveDateTime::default(),
            source_document_ids: Vec::new(),
        }
    }

    fn household(married: bool) -> Ownership {
        let accounts = [
            account("ope-shareworks", &["ope"]),
            account("ope-schwab", &["ope"]),
            account("tomi-schwab", &["tomi"]),
        ];
        let profiles = if married {
            [profile("ope", Some("tomi")), profile("tomi", Some("ope"))]
        } else {
            [profile("ope", None), profile("tomi", None)]
        };
        Ownership::new(&accounts, &profiles)
    }

    /// The August chain: Shareworks to Ope's Schwab a day later, then on to
    /// Tomi's Schwab two days after that, all 565 shares.
    fn august_chain() -> Vec<InvestmentEvent> {
        vec![
            transfer("sw-out", "ope-shareworks", "2026-08-10", -565),
            transfer("schwab-in", "ope-schwab", "2026-08-11", 565),
            transfer("schwab-out", "ope-schwab", "2026-08-13", -565),
            transfer("tomi-in", "tomi-schwab", "2026-08-13", 565),
        ]
    }

    #[test]
    fn a_move_between_one_persons_accounts_is_not_a_person_transfer() {
        let transfers = pair_person_transfers(&august_chain(), &household(true));

        assert!(!transfers.contains_key("sw-out"));
        assert!(!transfers.contains_key("schwab-in"));
        let out = &transfers["schwab-out"];
        assert!(out.outgoing);
        assert_eq!(out.counterpart_id, "tomi-in");
        assert_eq!((out.from.as_str(), out.to.as_str()), ("ope", "tomi"));
        assert!(!transfers["tomi-in"].outgoing);
    }

    #[test]
    fn the_rule_follows_the_recorded_spouse() {
        let married = pair_person_transfers(&august_chain(), &household(true));
        let unmarried = pair_person_transfers(&august_chain(), &household(false));

        assert_eq!(married["tomi-in"].rule, TransferRule::NoGainNoLoss);
        assert_eq!(unmarried["tomi-in"].rule, TransferRule::MarketValue);
    }

    #[test]
    fn sides_that_differ_in_size_or_arrive_too_late_or_early_stay_unpaired() {
        let events = vec![
            transfer("out-a", "ope-schwab", "2026-03-10", -45),
            transfer("in-a", "tomi-schwab", "2026-03-10", 40),
            transfer("out-b", "ope-schwab", "2026-06-15", -95),
            transfer("in-b", "tomi-schwab", "2026-06-23", 95),
            transfer("out-c", "ope-schwab", "2026-08-13", -565),
            transfer("in-c", "tomi-schwab", "2026-08-12", 565),
        ];

        assert!(pair_person_transfers(&events, &household(true)).is_empty());
    }

    #[test]
    fn a_report_on_the_receiver_needs_the_senders_ledger() {
        let transfers = pair_person_transfers(&august_chain(), &household(true));
        let tomi = HashSet::from(["tomi".to_string()]);
        let ope = HashSet::from(["ope".to_string()]);

        assert_eq!(
            owners_needed(&tomi, &transfers),
            HashSet::from(["tomi".to_string(), "ope".to_string()])
        );
        assert_eq!(owners_needed(&ope, &transfers), ope);
    }
}
