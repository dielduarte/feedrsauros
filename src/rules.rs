use serde::{Deserialize, Serialize};

/// What happens to a new article that matches a rule's condition.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RuleAction {
    Hide,
    KeepOnly,
}

/// A reader's filter on one feed, judged by Jev for each new article.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Rule {
    /// In the reader's own words, e.g. "soccer news".
    pub condition: String,
    pub action: RuleAction,
}

/// Whether a new article reaches the list, given whether it matched each rule (in order).
pub fn keeps(rules: &[Rule], matched: &[bool]) -> bool {
    let judged = || rules.iter().zip(matched);
    let hidden = judged().any(|(rule, &hit)| rule.action == RuleAction::Hide && hit);
    let mut keep_only = judged().filter(|(rule, _)| rule.action == RuleAction::KeepOnly);
    let wanted = keep_only.clone().next().is_none() || keep_only.any(|(_, &hit)| hit);
    !hidden && wanted
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rule(action: RuleAction) -> Rule {
        Rule {
            condition: "anything".into(),
            action,
        }
    }

    #[test]
    fn keeps_everything_without_rules() {
        assert!(keeps(&[], &[]));
    }

    #[test]
    fn hides_an_article_that_matches_a_hide_rule() {
        let rules = [rule(RuleAction::Hide), rule(RuleAction::Hide)];

        assert!(!keeps(&rules, &[false, true]));
        assert!(keeps(&rules, &[false, false]));
    }

    #[test]
    fn keeps_only_articles_that_match_some_keep_only_rule() {
        let rules = [rule(RuleAction::KeepOnly), rule(RuleAction::KeepOnly)];

        assert!(keeps(&rules, &[false, true]));
        assert!(!keeps(&rules, &[false, false]));
    }

    #[test]
    fn a_hide_rule_wins_over_a_keep_only_rule() {
        let rules = [rule(RuleAction::KeepOnly), rule(RuleAction::Hide)];

        assert!(!keeps(&rules, &[true, true]));
        assert!(keeps(&rules, &[true, false]));
    }
}
