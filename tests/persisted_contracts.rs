#![cfg(feature = "serde")]

use std::num::{NonZeroU16, NonZeroU32};

use tournamint::elimination::{
    EliminationKind, EliminationNodeDescriptor, EliminationNodeFact, EliminationNodeFactState,
    EliminationNodeId, EliminationNodeResolution, EliminationSource, EliminationStage,
};
use tournamint::round_robin::{
    KoyaOptions, RoundRobinCriterion, RoundRobinDirectEncounterOptions,
    RoundRobinProgressiveOptions, RoundRobinSonnebornBergerOptions, RoundRobinStandingsPlan,
};
use tournamint::series::{
    ClinchPolicy, EntrantSide, SeriesGameId, SeriesPhase, SeriesPlan, SetLimit,
};
use tournamint::standings::{
    CriterionValue, DirectEncounterForfeitPolicy, DirectEncounterOptions, ProgressiveScoreOptions,
    RepeatedEncounterPolicy, SonnebornBergerOptions, TournamentPairingNumberOrder,
};
use tournamint::swiss::{
    AccelerationPolicy, BuchholzOptions, BursteinConfig, ByeAssignment, ByeKind,
    DoubleSwissByePoints, DoubleSwissConfig, DoubleSwissPrimaryScore, DutchConfig,
    FlatAcceleration, MatchDisposition, RoundPairings, ScoreBasis, SingleGameSwissByePoints,
    SwissCriterion, SwissGameId, SwissLeg, SwissStandingsPlan, SwissSystemConfig,
    UnplayedRoundPolicy,
};
use tournamint::{MatchPointSystem, MatchScore, Pairing, PlayerId, PointSystem, Score};

// The JSON is written independently of serialization. Comparing JSON values
// ignores object-key order, but freezes field names, enum tags, numbers, and
// array order in both directions.
fn assert_contract<T>(value: &T, json: &str)
where
    T: serde::Serialize + serde::de::DeserializeOwned + PartialEq + core::fmt::Debug,
{
    let expected: serde_json::Value = serde_json::from_str(json).unwrap();
    assert_eq!(serde_json::to_value(value).unwrap(), expected);
    assert_eq!(serde_json::from_str::<T>(json).unwrap(), *value);
}

fn reject_unknown_at<T: serde::de::DeserializeOwned>(json: &str, pointers: &[&str]) {
    for pointer in pointers {
        let mut unknown: serde_json::Value = serde_json::from_str(json).unwrap();
        unknown
            .pointer_mut(pointer)
            .unwrap()
            .as_object_mut()
            .unwrap()
            .insert("unknown".into(), serde_json::Value::Null);
        assert!(serde_json::from_value::<T>(unknown).is_err(), "{pointer}");
    }
}

#[test]
fn every_swiss_system_and_acceleration_has_a_frozen_contract() {
    let single_bye = SingleGameSwissByePoints::game_win(PointSystem::STANDARD);
    let dutch = r#"{"dutch":{"bye_points":{"pairing_allocated":2}}}"#;
    assert_contract(
        &SwissSystemConfig::Dutch(DutchConfig {
            bye_points: single_bye,
        }),
        dutch,
    );
    reject_unknown_at::<SwissSystemConfig>(dutch, &["/dutch", "/dutch/bye_points"]);
    let burstein = r#"{"burstein":{"bye_points":{"pairing_allocated":2}}}"#;
    assert_contract(
        &SwissSystemConfig::Burstein(BursteinConfig {
            bye_points: single_bye,
        }),
        burstein,
    );
    reject_unknown_at::<SwissSystemConfig>(burstein, &["/burstein", "/burstein/bye_points"]);

    let double = DoubleSwissConfig {
        primary_score: DoubleSwissPrimaryScore::GamePoints,
        match_point_system: MatchPointSystem::STANDARD,
        bye_points: DoubleSwissByePoints::standard(
            PointSystem::STANDARD,
            MatchPointSystem::STANDARD,
        )
        .unwrap(),
    };
    let json = r#"{"double_swiss":{
        "primary_score":"game_points",
        "match_point_system":{"win":2,"draw":1,"loss":0,"forfeit_loss":0},
        "bye_points":{"game_points":3,"match_points":2,"counts_as_win":true}
    }}"#;
    assert_contract(&SwissSystemConfig::DoubleSwiss(double), json);
    reject_unknown_at::<SwissSystemConfig>(
        json,
        &[
            "/double_swiss",
            "/double_swiss/match_point_system",
            "/double_swiss/bye_points",
        ],
    );
    assert_contract(&DoubleSwissPrimaryScore::MatchPoints, r#""match_points""#);
    assert_contract(&AccelerationPolicy::None, r#""none""#);
    assert_contract(&AccelerationPolicy::Baku, r#""baku""#);
    let flat = r#"{"flat":{"rounds":2,"bonus":1}}"#;
    assert_contract(
        &AccelerationPolicy::Flat(FlatAcceleration {
            rounds: 2,
            bonus: Score::new(1),
        }),
        flat,
    );
    reject_unknown_at::<AccelerationPolicy>(flat, &["/flat"]);
}

#[test]
fn complete_swiss_standings_plan_freezes_every_criterion_and_option_shape() {
    let plan = SwissStandingsPlan::new(vec![
        SwissCriterion::PrimaryScore,
        SwissCriterion::GamePoints,
        SwissCriterion::MatchPoints,
        SwissCriterion::Buchholz(BuchholzOptions {
            cut_lowest: 1,
            cut_highest: 2,
            unplayed: UnplayedRoundPolicy::FideMarch2026,
        }),
        SwissCriterion::SonnebornBerger(SonnebornBergerOptions {
            score_basis: ScoreBasis::GamePoints,
            cut_lowest: 1,
            unplayed: UnplayedRoundPolicy::Exclude,
        }),
        SwissCriterion::Progressive(ProgressiveScoreOptions {
            score_basis: ScoreBasis::MatchPoints,
            cut_first_round: true,
        }),
        SwissCriterion::TournamentPairingNumber(TournamentPairingNumberOrder::Ascending),
        SwissCriterion::DirectEncounter(DirectEncounterOptions {
            score_basis: ScoreBasis::Primary,
            forfeits: DirectEncounterForfeitPolicy::IncludeAsScored,
            repeated_encounters: RepeatedEncounterPolicy::Sum,
        }),
        SwissCriterion::NumberOfWins,
        SwissCriterion::GamesWon,
        SwissCriterion::GamesWonWithBlack,
    ]);
    let json = r#"{"criteria":[
        "primary_score","game_points","match_points",
        {"buchholz":{"cut_lowest":1,"cut_highest":2,"unplayed":"fide_march2026"}},
        {"sonneborn_berger":{"score_basis":"game_points","cut_lowest":1,"unplayed":"exclude"}},
        {"progressive":{"score_basis":"match_points","cut_first_round":true}},
        {"tournament_pairing_number":"ascending"},
        {"direct_encounter":{"score_basis":"primary","forfeits":"include_as_scored","repeated_encounters":"sum"}},
        "number_of_wins","games_won","games_won_with_black"
    ]}"#;
    assert_contract(&plan, json);
    reject_unknown_at::<SwissStandingsPlan>(
        json,
        &[
            "",
            "/criteria/3/buchholz",
            "/criteria/4/sonneborn_berger",
            "/criteria/5/progressive",
            "/criteria/7/direct_encounter",
        ],
    );
    assert_contract(
        &SwissStandingsPlan::default(),
        r#"{"criteria":[
        "primary_score",
        {"buchholz":{"cut_lowest":0,"cut_highest":0,"unplayed":"fide_march2026"}},
        {"sonneborn_berger":{"score_basis":"primary","cut_lowest":0,"unplayed":"fide_march2026"}},
        {"progressive":{"score_basis":"primary","cut_first_round":false}},
        {"direct_encounter":{"score_basis":"primary","forfeits":"exclude","repeated_encounters":"average"}},
        "number_of_wins"
    ]}"#,
    );
}

#[test]
fn swiss_pairings_both_bye_kinds_and_every_game_leg_keep_their_wire_identity() {
    assert_contract(
        &RoundPairings {
            games: vec![Pairing::new(PlayerId::new(4), PlayerId::new(2))],
            byes: vec![
                ByeAssignment {
                    player: PlayerId::new(0),
                    kind: ByeKind::PairingAllocated,
                },
                ByeAssignment {
                    player: PlayerId::new(1),
                    kind: ByeKind::Requested,
                },
            ],
        },
        r#"{"games":[{"white":4,"black":2}],"byes":[
        {"player":0,"kind":"pairing_allocated"},{"player":1,"kind":"requested"}
    ]}"#,
    );
    for (leg, json) in [
        (
            SwissLeg::Single,
            r#"{"round_index":3,"pairing_index":7,"leg":"single"}"#,
        ),
        (
            SwissLeg::First,
            r#"{"round_index":3,"pairing_index":7,"leg":"first"}"#,
        ),
        (
            SwissLeg::Second,
            r#"{"round_index":3,"pairing_index":7,"leg":"second"}"#,
        ),
    ] {
        assert_contract(
            &SwissGameId {
                round_index: 3,
                pairing_index: 7,
                leg,
            },
            json,
        );
        reject_unknown_at::<SwissGameId>(json, &[""]);
    }
}

#[test]
fn round_robin_standings_freeze_every_criterion_and_option_shape() {
    let plan = RoundRobinStandingsPlan::new(vec![
        RoundRobinCriterion::PrimaryScore,
        RoundRobinCriterion::GamePoints,
        RoundRobinCriterion::MatchPoints,
        RoundRobinCriterion::MatchesWon,
        RoundRobinCriterion::TournamentPairingNumber(TournamentPairingNumberOrder::Descending),
        RoundRobinCriterion::Progressive(RoundRobinProgressiveOptions {
            cut_first_round: true,
        }),
        RoundRobinCriterion::DirectEncounter(RoundRobinDirectEncounterOptions {
            forfeits: DirectEncounterForfeitPolicy::Exclude,
            repeated_encounters: RepeatedEncounterPolicy::Average,
        }),
        RoundRobinCriterion::SonnebornBerger(RoundRobinSonnebornBergerOptions { cut_lowest: 1 }),
        RoundRobinCriterion::Koya(KoyaOptions {
            threshold_numerator: 1,
            threshold_denominator: NonZeroU32::new(2).unwrap(),
            include_adjudicated: false,
        }),
        RoundRobinCriterion::NumberOfWins,
        RoundRobinCriterion::GamesWon,
        RoundRobinCriterion::GamesWonWithBlack,
    ]);
    let json = r#"{"criteria":[
        "primary_score","game_points","match_points","matches_won",
        {"tournament_pairing_number":"descending"},
        {"progressive":{"cut_first_round":true}},
        {"direct_encounter":{"forfeits":"exclude","repeated_encounters":"average"}},
        {"sonneborn_berger":{"cut_lowest":1}},
        {"koya":{"threshold_numerator":1,"threshold_denominator":2,"include_adjudicated":false}},
        "number_of_wins","games_won","games_won_with_black"
    ]}"#;
    assert_contract(&plan, json);
    reject_unknown_at::<RoundRobinStandingsPlan>(
        json,
        &[
            "",
            "/criteria/5/progressive",
            "/criteria/6/direct_encounter",
            "/criteria/7/sonneborn_berger",
            "/criteria/8/koya",
        ],
    );
}

#[test]
fn elimination_kinds_stages_and_sources_have_frozen_tags() {
    assert_contract(
        &EliminationKind::Single { bronze: true },
        r#"{"single":{"bronze":true}}"#,
    );
    assert_contract(&EliminationKind::Double, r#""double""#);
    for (stage, json) in [
        (
            EliminationStage::SingleRound { round_index: 2 },
            r#"{"single_round":{"round_index":2}}"#,
        ),
        (EliminationStage::SingleFinal, r#""single_final""#),
        (EliminationStage::Bronze, r#""bronze""#),
        (
            EliminationStage::WinnersRound { round_index: 2 },
            r#"{"winners_round":{"round_index":2}}"#,
        ),
        (
            EliminationStage::LosersMinor { round_index: 2 },
            r#"{"losers_minor":{"round_index":2}}"#,
        ),
        (
            EliminationStage::LosersMajor { round_index: 2 },
            r#"{"losers_major":{"round_index":2}}"#,
        ),
        (EliminationStage::GrandFinal, r#""grand_final""#),
        (EliminationStage::Reset, r#""reset""#),
    ] {
        assert_contract(&stage, json);
    }
    for (source, json) in [
        (
            EliminationSource::InitialSeed { seed_index: 3 },
            r#"{"initial_seed":{"seed_index":3}}"#,
        ),
        (
            EliminationSource::WinnerOf(EliminationNodeId::new(7)),
            r#"{"winner_of":7}"#,
        ),
        (
            EliminationSource::LoserOf(EliminationNodeId::new(7)),
            r#"{"loser_of":7}"#,
        ),
        (EliminationSource::Vacant, r#""vacant""#),
    ] {
        assert_contract(&source, json);
    }
}

#[test]
fn every_elimination_resolution_and_fact_state_has_a_frozen_contract() {
    let first = PlayerId::new(4);
    let second = PlayerId::new(9);
    for (resolution, json, pointer) in [
        (
            EliminationNodeResolution::MatchDecided {
                winner: first,
                loser: second,
            },
            r#"{"match_decided":{"winner":4,"loser":9}}"#,
            Some("/match_decided"),
        ),
        (
            EliminationNodeResolution::WithdrawalWalkover {
                winner: first,
                withdrawn: second,
            },
            r#"{"withdrawal_walkover":{"winner":4,"withdrawn":9}}"#,
            Some("/withdrawal_walkover"),
        ),
        (
            EliminationNodeResolution::WithdrawalVacancy { withdrawn: first },
            r#"{"withdrawal_vacancy":{"withdrawn":4}}"#,
            Some("/withdrawal_vacancy"),
        ),
        (
            EliminationNodeResolution::MutualWithdrawalVacancy {
                withdrawn: [first, second],
            },
            r#"{"mutual_withdrawal_vacancy":{"withdrawn":[4,9]}}"#,
            Some("/mutual_withdrawal_vacancy"),
        ),
        (
            EliminationNodeResolution::AutomaticAdvance { player: first },
            r#"{"automatic_advance":{"player":4}}"#,
            Some("/automatic_advance"),
        ),
        (EliminationNodeResolution::Vacant, r#""vacant""#, None),
    ] {
        assert_contract(&resolution, json);
        if let Some(pointer) = pointer {
            reject_unknown_at::<EliminationNodeResolution>(json, &[pointer]);
        }
    }
    for (state, json) in [
        (
            EliminationNodeFactState::Planned,
            r#"{"node":3,"state":"planned"}"#,
        ),
        (
            EliminationNodeFactState::Active {
                entrants: [first, second],
            },
            r#"{"node":3,"state":{"active":{"entrants":[4,9]}}}"#,
        ),
        (
            EliminationNodeFactState::Resolved {
                entrants: [Some(first), None],
                resolution: EliminationNodeResolution::AutomaticAdvance { player: first },
            },
            r#"{"node":3,"state":{"resolved":{"entrants":[4,null],"resolution":{"automatic_advance":{"player":4}}}}}"#,
        ),
        (
            EliminationNodeFactState::Skipped,
            r#"{"node":3,"state":"skipped"}"#,
        ),
    ] {
        assert_contract(
            &EliminationNodeFact {
                node: EliminationNodeId::new(3),
                state,
            },
            json,
        );
        reject_unknown_at::<EliminationNodeFact>(json, &[""]);
    }
    reject_unknown_at::<EliminationNodeFactState>(r#"{"active":{"entrants":[4,9]}}"#, &["/active"]);
    reject_unknown_at::<EliminationNodeFactState>(
        r#"{"resolved":{"entrants":[4,null],"resolution":{"automatic_advance":{"player":4}}}}"#,
        &["/resolved", "/resolved/resolution/automatic_advance"],
    );
}

#[test]
fn elimination_descriptors_freeze_identity_and_topology_field_names() {
    let json = r#"{"id":5,"scheduled_wave_index":3,"stage":{"losers_major":{"round_index":1}},
        "stage_ordinal":2,"sources":[{"winner_of":1},{"loser_of":4}]}"#;
    assert_contract(
        &EliminationNodeDescriptor {
            id: EliminationNodeId::new(5),
            scheduled_wave_index: 3,
            stage: EliminationStage::LosersMajor { round_index: 1 },
            stage_ordinal: 2,
            sources: [
                EliminationSource::WinnerOf(EliminationNodeId::new(1)),
                EliminationSource::LoserOf(EliminationNodeId::new(4)),
            ],
        },
        json,
    );
    reject_unknown_at::<EliminationNodeDescriptor>(json, &["", "/stage/losers_major"]);
}

#[test]
fn series_plan_freezes_both_set_limits_clinch_policies_and_entrant_sides() {
    let plan = SeriesPlan {
        phases: vec![
            SeriesPhase {
                games_per_set: NonZeroU16::new(2).unwrap(),
                color_order: vec![EntrantSide::First, EntrantSide::Second],
                set_limit: SetLimit::AtMost(NonZeroU16::new(3).unwrap()),
                clinch: ClinchPolicy::EarlyClinch,
            },
            SeriesPhase {
                games_per_set: NonZeroU16::new(2).unwrap(),
                color_order: vec![EntrantSide::Second, EntrantSide::First],
                set_limit: SetLimit::UntilDecisive,
                clinch: ClinchPolicy::PlayAll,
            },
        ],
    };
    let json = r#"{"phases":[
        {"games_per_set":2,"color_order":["first","second"],"set_limit":{"at_most":3},"clinch":"early_clinch"},
        {"games_per_set":2,"color_order":["second","first"],"set_limit":"until_decisive","clinch":"play_all"}
    ]}"#;
    assert_contract(&plan, json);
    reject_unknown_at::<SeriesPlan>(json, &["", "/phases/0", "/phases/1"]);
    assert_contract(&SeriesGameId::new(17), "17");
}

#[test]
fn shared_standings_values_and_match_results_keep_their_tags() {
    for (value, json) in [
        (CriterionValue::Score(Score::new(3)), r#"{"score":3}"#),
        (CriterionValue::Integer(9), r#"{"integer":9}"#),
        (CriterionValue::NotApplicable, r#""not_applicable""#),
    ] {
        assert_contract(&value, json);
    }
    for (score, json) in [
        (MatchScore::Win, r#""win""#),
        (MatchScore::Draw, r#""draw""#),
        (MatchScore::Loss, r#""loss""#),
    ] {
        assert_contract(&score, json);
    }
    for (disposition, json) in [
        (MatchDisposition::Regular, r#""regular""#),
        (MatchDisposition::ForfeitWin, r#""forfeit_win""#),
        (MatchDisposition::ForfeitLoss, r#""forfeit_loss""#),
        (MatchDisposition::DoubleForfeit, r#""double_forfeit""#),
    ] {
        assert_contract(&disposition, json);
    }
}
