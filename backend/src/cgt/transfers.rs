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

use chrono::{Duration, NaiveDate};
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
    pub(crate) symbol: String,
    /// The departing side's date, price per share and currency. A gift's
    /// market value comes from here on both sides, so the giver's proceeds and
    /// the receiver's cost are one figure whatever the arriving row says.
    pub(crate) sent_on: NaiveDate,
    pub(crate) sent_price: Decimal,
    pub(crate) sent_currency: String,
}

impl PersonTransfer {
    /// A gift whose market value cannot be established, because the departing
    /// side has no price. The import is told to use 0 for a price that does
    /// not apply, so transfer rows often carry one.
    pub(crate) fn is_unpriced_gift(&self) -> bool {
        self.rule == TransferRule::MarketValue && self.sent_price <= Decimal::ZERO
    }
}

/// Pair each departing `transfer` (negative quantity) with an arriving one of
/// the same symbol and size, on the same day or at most
/// [`TRANSFER_PAIRING_DAYS`] later. Days are compared, not timestamps, so an
/// arrival stamped earlier in the day than its departure still pairs.
///
/// A departure on the arrival's own day is preferred over any other, and only
/// then the closest earlier one. Within each of those, moves between one
/// person's own accounts are paired first, so they are never mistaken for a
/// transfer to someone else. Without the same-day preference, a move between
/// one person's own accounts, sent on the 1st and arriving on the 5th, took
/// the closer departure of their gift on the 3rd, leaving the gift paired with
/// the 1st's departure, at the wrong date and price.
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
    let passes = [(true, true), (true, false), (false, true), (false, false)];
    for (same_day, same_person) in passes {
        for (i, (arrival, receiver)) in ins.iter().enumerate() {
            if used_in[i] {
                continue;
            }
            let arrived = arrival.date.date();
            let departure = outs
                .iter()
                .enumerate()
                .filter(|(o, (out, sender))| {
                    let sent = out.date.date();
                    !used_out[*o]
                        && (sender == receiver) == same_person
                        && out.symbol == arrival.symbol
                        && -out.quantity == arrival.quantity
                        && sent <= arrived
                        && arrived - sent <= Duration::days(TRANSFER_PAIRING_DAYS)
                        && (sent == arrived || !same_day)
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
                symbol: out.symbol.clone(),
                sent_on: out.date.date(),
                sent_price: out.price_per_share,
                sent_currency: out.currency.clone(),
            };
            transfers.insert(out.id.clone(), leg(true, arrival));
            transfers.insert(arrival.id.clone(), leg(false, out));
        }
    }
    transfers
}

/// What a report on `owners` needs beyond their own events: for each symbol a
/// spouse moved to one of them, the sender's events in that symbol, since the
/// receiver takes over the sender's cost. Transitive, as the sender's pool may
/// in turn hold shares from their own spouse. Returned as (sender, symbol).
///
/// A gift needs nothing of the giver's, because its market value travels with
/// the transfer, and a spouse's other symbols play no part in the cost. Pulling
/// in more than this would let an unrelated gap in the sender's ledger, such as
/// a missing exchange rate on another holding, refuse the receiver's report.
pub(crate) fn spouse_holdings_needed(
    owners: &HashSet<String>,
    transfers: &HashMap<String, PersonTransfer>,
) -> HashSet<(String, String)> {
    fn holds(
        owners: &HashSet<String>,
        needed: &HashSet<(String, String)>,
        owner: &str,
        symbol: &str,
    ) -> bool {
        owners.contains(owner) || needed.contains(&(owner.to_string(), symbol.to_string()))
    }
    let mut needed = HashSet::new();
    loop {
        let senders: Vec<(String, String)> = transfers
            .values()
            .filter(|t| t.rule == TransferRule::NoGainNoLoss)
            .filter(|t| {
                holds(owners, &needed, &t.to, &t.symbol)
                    && !holds(owners, &needed, &t.from, &t.symbol)
            })
            .map(|t| (t.from.clone(), t.symbol.clone()))
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
    /// those of [`spouse_holdings_needed`]. A scoped account with several
    /// owners keeps its events, for `check_single_owner_accounts` to refuse.
    /// `excluded_accounts` (ISAs, pensions) never take part.
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
                let from_spouses = spouse_holdings_needed(&scoped_owners, &transfers);
                events
                    .into_iter()
                    .filter(|e| match ownership.owner(&e.account_id) {
                        Some(owner) => {
                            scoped_owners.contains(owner)
                                || from_spouses.contains(&(owner.to_string(), e.symbol.clone()))
                        }
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

    /// `date` is `YYYY-MM-DD` (midnight) or `YYYY-MM-DDTHH:MM:SS`.
    fn transfer(id: &str, account: &str, date: &str, quantity: i64) -> InvestmentEvent {
        let date = if date.contains('T') {
            date.to_string()
        } else {
            format!("{date}T00:00:00")
        };
        InvestmentEvent {
            id: id.to_string(),
            account_id: account.to_string(),
            event_type: InvestmentEventType::Transfer,
            symbol: "PLTR".to_string(),
            date: NaiveDateTime::parse_from_str(&date, "%Y-%m-%dT%H:%M:%S").unwrap(),
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
    fn an_arrival_stamped_earlier_in_the_day_than_its_departure_still_pairs() {
        let events = vec![
            transfer("out", "ope-schwab", "2026-08-13T15:30:00", -565),
            transfer("in", "tomi-schwab", "2026-08-13T09:00:00", 565),
            transfer("late-out", "ope-schwab", "2026-09-01T08:00:00", -10),
            transfer("late-in", "tomi-schwab", "2026-09-08T23:00:00", 10),
        ];

        let transfers = pair_person_transfers(&events, &household(true));

        assert_eq!(transfers["in"].counterpart_id, "out");
        assert_eq!(
            transfers["late-in"].counterpart_id, "late-out",
            "seven days apart by date, though more than seven by the clock"
        );
    }

    #[test]
    fn a_departure_on_the_arrivals_own_day_wins_over_a_closer_own_account_move() {
        // Ope moves 100 between his own accounts (sent 1 Aug, arriving 5 Aug)
        // and gives 100 to Tomi on 3 Aug. The own-account arrival is closer to
        // the 3 Aug departure, which used to take it and leave the gift paired
        // with the 1 Aug departure.
        let mut events = vec![
            transfer("move-out", "ope-shareworks", "2026-08-01", -100),
            transfer("move-in", "ope-schwab", "2026-08-05", 100),
            transfer("gift-out", "ope-shareworks", "2026-08-03", -100),
            transfer("gift-in", "tomi-schwab", "2026-08-03", 100),
        ];
        events[0].price_per_share = Decimal::from(120);
        events[2].price_per_share = Decimal::from(130);

        let transfers = pair_person_transfers(&events, &household(false));

        let gift = &transfers["gift-in"];
        assert_eq!(gift.counterpart_id, "gift-out");
        assert_eq!(gift.sent_on.to_string(), "2026-08-03");
        assert_eq!(gift.sent_price, Decimal::from(130));
        assert!(!transfers.contains_key("move-out"));
        assert!(!transfers.contains_key("move-in"));
    }

    #[test]
    fn a_report_on_the_receiver_needs_only_the_spouses_holding_in_that_symbol() {
        let married = pair_person_transfers(&august_chain(), &household(true));
        let unmarried = pair_person_transfers(&august_chain(), &household(false));
        let tomi = HashSet::from(["tomi".to_string()]);
        let ope = HashSet::from(["ope".to_string()]);

        assert_eq!(
            spouse_holdings_needed(&tomi, &married),
            HashSet::from([("ope".to_string(), "PLTR".to_string())])
        );
        assert!(spouse_holdings_needed(&ope, &married).is_empty());
        assert!(
            spouse_holdings_needed(&tomi, &unmarried).is_empty(),
            "a gift carries its own market value"
        );
    }

    #[test]
    fn a_report_on_the_receiver_leaves_out_the_senders_other_symbols() {
        let mut msft = transfer("msft-buy", "ope-schwab", "2026-01-05", 0);
        msft.event_type = InvestmentEventType::Buy;
        msft.symbol = "MSFT".to_string();
        msft.quantity = Decimal::from(5);
        let mut events = august_chain();
        events.push(msft);
        let scope = HashSet::from(["tomi-schwab".to_string()]);

        let married = Ledger::build(
            events.clone(),
            household(true),
            Some(scope.clone()),
            &HashSet::new(),
        );
        let unmarried = Ledger::build(events, household(false), Some(scope), &HashSet::new());

        let ids = |ledger: &Ledger| -> Vec<String> {
            let mut ids: Vec<String> = ledger.events.iter().map(|e| e.id.clone()).collect();
            ids.sort();
            ids
        };
        assert_eq!(
            ids(&married),
            ["schwab-in", "schwab-out", "sw-out", "tomi-in"]
        );
        assert_eq!(ids(&unmarried), ["tomi-in"]);
    }
}
