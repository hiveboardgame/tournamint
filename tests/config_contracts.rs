#![cfg(feature = "serde")]

use tournamint::arena::{ArenaConfig, ArenaPairingPolicy};
use tournamint::round_robin::{
    RoundRobinConfig, RoundRobinCriterion, RoundRobinPrimaryScore, RoundRobinStandingsPlan,
};
use tournamint::{Color, MatchPointSystem, PointSystem};

fn assert_contract<T>(value: &T, json: &str)
where
    T: serde::Serialize + serde::de::DeserializeOwned + PartialEq + core::fmt::Debug,
{
    let expected: serde_json::Value = serde_json::from_str(json).unwrap();
    assert_eq!(serde_json::to_value(value).unwrap(), expected);
    assert_eq!(serde_json::from_str::<T>(json).unwrap(), *value);
}

#[test]
fn arena_rules_have_a_frozen_persisted_contract() {
    let config = ArenaConfig::default();
    let json = r#"{
        "scoring":{"fire":true,"berserk":{"win_bonus":1,"minimum_bonus_plies":14},
            "quick_draw":{"minimum_scoring_plies":20},
            "repeated_draw":{"minimum_unsuppressed_plies":60}},
        "pairing_policy":"grouped","rematch_cooldown":1,"rematch_wait_secs":15,"max_color_strike":3,
        "initial_color":"white"
    }"#;
    assert_contract(&config, json);
    assert_contract(
        &ArenaConfig::plain(),
        r#"{
        "scoring":{"fire":false,"berserk":null,"quick_draw":null,"repeated_draw":null},
        "pairing_policy":"grouped","rematch_cooldown":1,"rematch_wait_secs":15,"max_color_strike":3,
        "initial_color":"white"
    }"#,
    );
    assert_contract(&ArenaPairingPolicy::Greedy, r#""greedy""#);

    let mut missing_wait: serde_json::Value = serde_json::from_str(json).unwrap();
    missing_wait
        .as_object_mut()
        .unwrap()
        .remove("rematch_wait_secs");
    assert!(serde_json::from_value::<ArenaConfig>(missing_wait).is_err());
    for wait in [8, 50] {
        let mut saved: serde_json::Value = serde_json::from_str(json).unwrap();
        saved["rematch_wait_secs"] = wait.into();
        let decoded: ArenaConfig = serde_json::from_value(saved.clone()).unwrap();
        assert_eq!(decoded.rematch_wait_secs, wait);
        assert_eq!(serde_json::to_value(decoded).unwrap(), saved);
        assert!(decoded.validate().is_ok());
    }

    for pointer in [
        "",
        "/scoring",
        "/scoring/berserk",
        "/scoring/quick_draw",
        "/scoring/repeated_draw",
    ] {
        let mut unknown: serde_json::Value = serde_json::from_str(json).unwrap();
        unknown
            .pointer_mut(pointer)
            .unwrap()
            .as_object_mut()
            .unwrap()
            .insert("unknown".into(), serde_json::Value::Null);
        assert!(
            serde_json::from_value::<ArenaConfig>(unknown).is_err(),
            "{pointer}"
        );
    }
    let mut invalid = config;
    invalid.max_color_strike = 0;
    assert!(invalid.validate().is_err());
}

#[test]
fn native_round_robin_rules_have_a_frozen_persisted_contract() {
    let config = RoundRobinConfig {
        repeats: 2,
        initial_color: Color::Black,
        point_system: PointSystem::STANDARD,
        primary_score: RoundRobinPrimaryScore::GamePoints,
        match_point_system: MatchPointSystem::STANDARD,
        standings: RoundRobinStandingsPlan::new(vec![RoundRobinCriterion::PrimaryScore]),
    };
    assert_contract(
        &config,
        r#"{
        "repeats":2,"initial_color":"black",
        "point_system":{"win":2,"draw":1,"loss":0,"forfeit_loss":0},
        "primary_score":"game_points",
        "match_point_system":{"win":2,"draw":1,"loss":0,"forfeit_loss":0},
        "standings":{"criteria":["primary_score"]}
    }"#,
    );
    let mut unknown = serde_json::to_value(config).unwrap();
    unknown
        .as_object_mut()
        .unwrap()
        .insert("unknown".into(), serde_json::Value::Null);
    assert!(serde_json::from_value::<RoundRobinConfig>(unknown).is_err());
}
