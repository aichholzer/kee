use criterion::{black_box, criterion_group, criterion_main, BenchmarkId, Criterion};
use kee::{KeeConfig, ProfileInfo};

/// Build a representative ProfileInfo with a unique suffix.
fn make_profile(i: usize) -> ProfileInfo {
    ProfileInfo {
        profile_name: format!("kee-profile-{i}"),
        sso_start_url: format!("https://acme-{i}.awsapps.com/start"),
        sso_region: "ap-southeast-2".to_string(),
        sso_account_id: format!("{:012}", 100000000000u64 + i as u64),
        sso_role_name: "AdministratorAccess".to_string(),
        session_name: format!("acme-session-{i}"),
        production: i % 3 == 0,
    }
}

/// Build a KeeConfig populated with `n` profiles.
fn make_config(n: usize) -> KeeConfig {
    let mut config = KeeConfig::new();
    for i in 0..n {
        config.add_profile(format!("profile-{i}"), make_profile(i));
    }
    if n > 0 {
        config.set_current_profile(Some(format!("profile-{}", n / 2)));
    }
    config
}

fn bench_profile_serialization(c: &mut Criterion) {
    let profile = make_profile(42);
    c.bench_function("profile_info_serialize", |b| {
        b.iter(|| serde_json::to_string(black_box(&profile)).unwrap())
    });

    let json = serde_json::to_string(&profile).unwrap();
    c.bench_function("profile_info_deserialize", |b| {
        b.iter(|| {
            let p: ProfileInfo = serde_json::from_str(black_box(&json)).unwrap();
            p
        })
    });
}

fn bench_config_build(c: &mut Criterion) {
    let mut group = c.benchmark_group("config_build");
    for size in [10usize, 100, 1000] {
        group.bench_with_input(BenchmarkId::from_parameter(size), &size, |b, &size| {
            b.iter(|| make_config(black_box(size)))
        });
    }
    group.finish();
}

fn bench_config_lookup(c: &mut Criterion) {
    let config = make_config(1000);
    c.bench_function("config_get_profile", |b| {
        b.iter(|| black_box(config.get_profile(black_box("profile-750"))))
    });

    c.bench_function("config_list_profiles", |b| {
        b.iter(|| black_box(config.list_profiles()))
    });
}

fn bench_config_serialization(c: &mut Criterion) {
    let mut group = c.benchmark_group("config_serialization");
    for size in [10usize, 100, 1000] {
        let config = make_config(size);
        group.bench_with_input(BenchmarkId::new("serialize", size), &config, |b, config| {
            b.iter(|| serde_json::to_string(black_box(config)).unwrap())
        });

        let json = serde_json::to_string(&config).unwrap();
        group.bench_with_input(BenchmarkId::new("deserialize", size), &json, |b, json| {
            b.iter(|| {
                let c: KeeConfig = serde_json::from_str(black_box(json)).unwrap();
                c
            })
        });
    }
    group.finish();
}

fn bench_config_remove(c: &mut Criterion) {
    c.bench_function("config_remove_profile", |b| {
        b.iter_batched(
            || make_config(100),
            |mut config| black_box(config.remove_profile(black_box("profile-50"))),
            criterion::BatchSize::SmallInput,
        )
    });
}

criterion_group!(
    benches,
    bench_profile_serialization,
    bench_config_build,
    bench_config_lookup,
    bench_config_serialization,
    bench_config_remove
);
criterion_main!(benches);
