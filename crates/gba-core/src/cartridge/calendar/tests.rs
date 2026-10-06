use super::*;

#[test]
fn calendar_validates_fields_and_epoch_range() {
    for invalid in [
        [100, 1, 1, 0, 0, 0, 0],
        [0, 0, 1, 0, 0, 0, 0],
        [0, 13, 1, 0, 0, 0, 0],
        [0, 1, 0, 0, 0, 0, 0],
        [1, 2, 29, 0, 0, 0, 0],
        [0, 4, 31, 0, 0, 0, 0],
        [0, 1, 1, 7, 0, 0, 0],
        [0, 1, 1, 0, 24, 0, 0],
        [0, 1, 1, 0, 0, 60, 0],
        [0, 1, 1, 0, 0, 0, 60],
    ] {
        assert_eq!(RtcDateTime::new(invalid), Err(RtcError::InvalidDateTime));
    }
    assert_eq!(
        RtcDateTime::from_seconds_since_2000(0)
            .unwrap()
            .components(),
        [0, 1, 1, 6, 0, 0, 0]
    );
    assert_eq!(
        RtcDateTime::from_seconds_since_2000(3_155_759_999)
            .unwrap()
            .components(),
        [99, 12, 31, 4, 23, 59, 59]
    );
    assert_eq!(
        RtcDateTime::from_seconds_since_2000(3_155_760_000),
        Err(RtcError::InvalidDateTime)
    );
    assert_eq!(
        RtcDateTime::from_seconds_since_2000(u64::MAX),
        Err(RtcError::InvalidDateTime)
    );
}

#[test]
fn hour_modes_cover_every_second_and_pm_flag() {
    for hour in 0..24 {
        for minute in 0..60 {
            for second in 0..60 {
                let date = RtcDateTime::new([24, 2, 29, 3, hour, minute, second]).unwrap();
                for mode in [false, true] {
                    let bytes = date.encode(mode);
                    assert_eq!(bytes[4] & 128 != 0, hour >= 12);
                    assert_eq!(RtcDateTime::decode(bytes, mode).unwrap(), date);
                }
            }
        }
    }
    assert_eq!(
        RtcDateTime::new([0, 1, 1, 0, 12, 0, 0])
            .unwrap()
            .encode(false)[4],
        0x80
    );
    assert_eq!(
        RtcDateTime::new([0, 1, 1, 0, 23, 0, 0])
            .unwrap()
            .encode(true)[4],
        0xa3
    );
    assert!(RtcDateTime::decode([0, 1, 1, 0, 0x12, 0, 0], false).is_err());
    assert_eq!(
        RtcDateTime::decode([0, 1, 1, 0, 0x80, 0, 0], true)
            .unwrap()
            .components()[4],
        0
    );
}

#[test]
fn weekday_advances_independently_and_all_months_roll_over() {
    // Independent month table and explicit leap-year set; test every day of the century.
    let leaps = [
        0, 4, 8, 12, 16, 20, 24, 28, 32, 36, 40, 44, 48, 52, 56, 60, 64, 68, 72, 76, 80, 84, 88,
        92, 96,
    ];
    let mut clock = RtcDateTime::new([0, 1, 1, 2, 0, 0, 0]).unwrap();
    let mut week = 2;
    for year in 0..100 {
        let months = [
            31,
            if leaps.contains(&year) { 29 } else { 28 },
            31,
            30,
            31,
            30,
            31,
            31,
            30,
            31,
            30,
            31,
        ];
        for (month, length) in months.into_iter().enumerate() {
            for day in 1..=length {
                assert_eq!(
                    clock.components(),
                    [year, month as u8 + 1, day, week, 0, 0, 0]
                );
                assert_eq!(
                    RtcDateTime::decode(clock.encode(true), true).unwrap(),
                    clock
                );
                clock.advance(86400);
                week = (week + 1) % 7;
            }
        }
    }
    assert_eq!(clock.components(), [0, 1, 1, 1, 0, 0, 0]);
}

#[test]
fn large_and_split_advances_do_not_overflow_or_lose_time() {
    let mut huge = RtcDateTime::default();
    huge.advance(u64::MAX);
    // Independent Python datetime calculation using the documented two-digit-year wrap policy.
    assert_eq!(huge.components(), [90, 8, 17, 0, 7, 0, 15]);
    let mut split = RtcDateTime::default();
    split.advance(u64::MAX / 2);
    split.advance(u64::MAX - u64::MAX / 2);
    assert_eq!(huge, split);
    for start in [
        [0, 2, 28, 6, 23, 59, 59],
        [1, 2, 28, 5, 23, 59, 59],
        [99, 12, 31, 2, 23, 59, 59],
    ] {
        let mut batched = RtcDateTime::new(start).unwrap();
        let mut small = batched;
        batched.advance(100000);
        for _ in 0..100000 {
            small.advance(1);
        }
        assert_eq!(batched, small);
    }
}
