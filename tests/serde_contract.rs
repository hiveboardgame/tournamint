#![cfg(feature = "serde")]

use core::num::{NonZeroU16, NonZeroU32};

use tournamint::elimination::{
    EliminationKind, EliminationNodeDescriptor, EliminationNodeFact, EliminationNodeFactState,
    EliminationNodeId, EliminationNodePatch, EliminationNodeResolution, EliminationReleaseCadence,
    EliminationReleasePatch, EliminationSource, EliminationStage, EliminationWavePatch,
};
use tournamint::round_robin::KoyaOptions;
use tournamint::round_robin::{
    RoundRobinCriterion, RoundRobinDirectEncounterOptions, RoundRobinGameId,
    RoundRobinStandingsPlan, ScheduledRoundRobinGame,
};
use tournamint::series::{ClinchPolicy, EntrantSide, SeriesPhase, SeriesPlan, SetLimit};
use tournamint::standings::{
    DirectEncounterForfeitPolicy, RepeatedEncounterPolicy, TournamentPairingNumberOrder,
};
use tournamint::swiss::{
    AccelerationPolicy, BuchholzOptions, ByeAssignment, ByeKind, DutchConfig, FlatAcceleration,
    RoundPairings, ScoreBasis, SingleGameSwissByePoints, SwissCriterion, SwissStandingsPlan,
    SwissSystemConfig, UnplayedRoundPolicy,
};
use tournamint::{
    AdjudicatedGameOutcome, AdjudicatedSideResult as A, Color, GameOutcome, Pairing,
    PlayedGameOutcome, PlayerId, PointSystem, Score,
};

fn round_trip_and_reject_unknown<T>(value: &T)
where
    T: serde::Serialize + serde::de::DeserializeOwned + Eq + core::fmt::Debug,
{
    let encoded = serde_json::to_value(value).unwrap();
    assert_eq!(
        serde_json::from_value::<T>(encoded.clone()).unwrap(),
        *value
    );
    let mut unknown = encoded;
    unknown
        .as_object_mut()
        .expect("persisted fixture is a struct")
        .insert("unknown".into(), serde_json::Value::Null);
    assert!(serde_json::from_value::<T>(unknown).is_err());
}

#[test]
fn game_outcome_has_one_closed_scalar_contract() {
    let adjudicated =
        |white, black| GameOutcome::Adjudicated(AdjudicatedGameOutcome::new(white, black).unwrap());
    let cases = [
        (
            "played_white_win",
            GameOutcome::Played(PlayedGameOutcome::WhiteWin),
            true,
        ),
        (
            "played_draw",
            GameOutcome::Played(PlayedGameOutcome::Draw),
            true,
        ),
        (
            "played_black_win",
            GameOutcome::Played(PlayedGameOutcome::BlackWin),
            true,
        ),
        (
            "adjudicated_white_forfeit_win",
            adjudicated(A::ForfeitWin, A::ForfeitLoss),
            false,
        ),
        (
            "adjudicated_black_forfeit_win",
            adjudicated(A::ForfeitLoss, A::ForfeitWin),
            false,
        ),
        ("adjudicated_draw", adjudicated(A::Draw, A::Draw), false),
        (
            "adjudicated_white_draw_black_forfeit_loss",
            adjudicated(A::Draw, A::ForfeitLoss),
            false,
        ),
        (
            "adjudicated_white_forfeit_loss_black_draw",
            adjudicated(A::ForfeitLoss, A::Draw),
            false,
        ),
        (
            "adjudicated_double_forfeit",
            adjudicated(A::DoubleForfeit, A::DoubleForfeit),
            false,
        ),
    ];

    for (token, outcome, played) in cases {
        assert_eq!(
            serde_json::to_string(&outcome).unwrap(),
            format!("\"{token}\"")
        );
        assert_eq!(
            serde_json::from_str::<GameOutcome>(&format!("\"{token}\"")).unwrap(),
            outcome
        );
        assert_eq!(
            serde_json::from_value::<GameOutcome>(serde_json::Value::String(token.to_owned()))
                .unwrap(),
            outcome
        );
        assert_eq!(outcome.counts_as_played(), played);
        assert_eq!(outcome.white().counts_as_played(), played);
        assert_eq!(outcome.black().counts_as_played(), played);
    }
    assert!(serde_json::from_str::<GameOutcome>("\"white_win\"").is_err());
    assert!(AdjudicatedGameOutcome::new(A::ForfeitWin, A::Draw).is_err());
}

#[test]
fn native_round_pairings_preserve_order_and_validate_the_union() {
    let json = r#"{"games":[{"white":3,"black":1},{"white":4,"black":2}],"byes":[{"player":0,"kind":"requested"}]}"#;
    let round: RoundPairings = serde_json::from_str(json).unwrap();
    assert_eq!(
        round.games,
        vec![
            Pairing::new(PlayerId::new(3), PlayerId::new(1)),
            Pairing::new(PlayerId::new(4), PlayerId::new(2))
        ]
    );
    assert_eq!(
        round.byes,
        vec![ByeAssignment {
            player: PlayerId::new(0),
            kind: ByeKind::Requested
        }]
    );
    assert_eq!(serde_json::to_string(&round).unwrap(), json);

    assert!(
        serde_json::from_str::<RoundPairings>(
            r#"{"games":[{"white":1,"black":2}],"byes":[{"player":2,"kind":"requested"}]}"#
        )
        .is_err()
    );
    assert!(
        serde_json::from_str::<RoundPairings>(r#"{"games":[],"byes":[],"eligible":[]}"#).is_err()
    );
}

#[test]
fn koya_denominator_is_nonzero_at_the_serde_boundary() {
    let options = KoyaOptions {
        threshold_numerator: 1,
        threshold_denominator: NonZeroU32::new(2).unwrap(),
        include_adjudicated: true,
    };
    assert_eq!(
        serde_json::from_str::<KoyaOptions>(&serde_json::to_string(&options).unwrap()).unwrap(),
        options
    );
    assert!(
        serde_json::from_str::<KoyaOptions>(
            r#"{"threshold_numerator":1,"threshold_denominator":0,"include_adjudicated":true}"#
        )
        .is_err()
    );
    assert!(serde_json::from_str::<KoyaOptions>(r#"{"threshold_numerator":1,"threshold_denominator":2,"include_adjudicated":true,"extra":0}"#).is_err());
}

#[test]
fn common_standings_and_round_robin_nested_contracts() {
    let points = PointSystem {
        win: Score::new(3),
        draw: Score::new(1),
        loss: Score::new(0),
        forfeit_loss: Score::new(0),
    };
    round_trip_and_reject_unknown(&points);
    assert_eq!(serde_json::to_string(&PlayerId::new(7)).unwrap(), "7");
    assert_eq!(serde_json::to_string(&Color::Black).unwrap(), r#""black""#);

    let plan = RoundRobinStandingsPlan::new(vec![
        RoundRobinCriterion::TournamentPairingNumber(TournamentPairingNumberOrder::Descending),
        RoundRobinCriterion::DirectEncounter(RoundRobinDirectEncounterOptions {
            forfeits: DirectEncounterForfeitPolicy::IncludeAsScored,
            repeated_encounters: RepeatedEncounterPolicy::Sum,
        }),
        RoundRobinCriterion::Koya(KoyaOptions::default()),
    ]);
    round_trip_and_reject_unknown(&plan);

    let game = ScheduledRoundRobinGame {
        id: RoundRobinGameId::new(9),
        round_index: 4,
        pass_index: 1,
        board_index: 2,
        white: PlayerId::new(3),
        black: PlayerId::new(8),
        previous_repeat: Some(RoundRobinGameId::new(1)),
        next_repeat: None,
    };
    round_trip_and_reject_unknown(&game);
}

#[test]
fn swiss_system_criteria_and_accepted_round_contract() {
    let system = SwissSystemConfig::Dutch(DutchConfig {
        bye_points: SingleGameSwissByePoints::game_win(PointSystem::STANDARD),
    });
    assert_eq!(
        serde_json::from_value::<SwissSystemConfig>(serde_json::to_value(system).unwrap()).unwrap(),
        system
    );
    let acceleration = AccelerationPolicy::Flat(FlatAcceleration {
        rounds: 2,
        bonus: Score::new(1),
    });
    assert_eq!(
        serde_json::from_value::<AccelerationPolicy>(serde_json::to_value(acceleration).unwrap())
            .unwrap(),
        acceleration
    );
    let plan = SwissStandingsPlan::new(vec![
        SwissCriterion::Buchholz(BuchholzOptions {
            cut_lowest: 1,
            cut_highest: 0,
            unplayed: UnplayedRoundPolicy::FideMarch2026,
        }),
        SwissCriterion::Progressive(tournamint::standings::ProgressiveScoreOptions {
            score_basis: ScoreBasis::Primary,
            cut_first_round: true,
        }),
    ]);
    round_trip_and_reject_unknown(&plan);
}

#[test]
fn elimination_descriptor_facts_resolutions_and_patches_contract() {
    let descriptor = EliminationNodeDescriptor {
        id: EliminationNodeId::new(4),
        scheduled_wave_index: 2,
        stage: EliminationStage::LosersMajor { round_index: 1 },
        stage_ordinal: 0,
        sources: [
            EliminationSource::WinnerOf(EliminationNodeId::new(2)),
            EliminationSource::LoserOf(EliminationNodeId::new(3)),
        ],
    };
    round_trip_and_reject_unknown(&descriptor);
    let state = EliminationNodeFactState::Resolved {
        entrants: [Some(PlayerId::new(10)), Some(PlayerId::new(11))],
        resolution: EliminationNodeResolution::WithdrawalWalkover {
            winner: PlayerId::new(10),
            withdrawn: PlayerId::new(11),
        },
    };
    round_trip_and_reject_unknown(&EliminationNodeFact {
        node: descriptor.id,
        state,
    });
    let patch = EliminationWavePatch {
        wave_index: descriptor.scheduled_wave_index,
        patches: vec![EliminationNodePatch {
            node: descriptor.id,
            state,
        }],
    };
    round_trip_and_reject_unknown(&patch);
    round_trip_and_reject_unknown(&EliminationReleasePatch {
        patches: patch.patches.clone(),
    });
    assert_eq!(
        serde_json::to_string(&EliminationReleaseCadence::Wave).unwrap(),
        r#""wave""#
    );
    assert_eq!(
        serde_json::to_string(&EliminationReleaseCadence::DependencyReady).unwrap(),
        r#""dependency_ready""#
    );
    assert!(serde_json::from_str::<EliminationReleaseCadence>(r#""rolling""#).is_err());
    assert!(EliminationKind::Double.supports_stage(descriptor.stage));
    assert!(!EliminationKind::Single { bronze: false }.supports_stage(EliminationStage::Bronze));
}

#[test]
fn series_plan_nested_contract() {
    let plan = SeriesPlan {
        phases: vec![SeriesPhase {
            games_per_set: NonZeroU16::new(2).unwrap(),
            color_order: vec![EntrantSide::First, EntrantSide::Second],
            set_limit: SetLimit::UntilDecisive,
            clinch: ClinchPolicy::EarlyClinch,
        }],
    };
    round_trip_and_reject_unknown(&plan);
}
