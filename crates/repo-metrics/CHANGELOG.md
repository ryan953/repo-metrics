# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.1.0](https://github.com/ryan953/repo-metrics/releases/tag/v0.1.0) - 2026-07-03

### Added

- *(db)* Add parent_sha column to stats table

### Other

- Add build workflow and release automation ([#1](https://github.com/ryan953/repo-metrics/pull/1))
- Apply cargo clippy fixes ([#3](https://github.com/ryan953/repo-metrics/pull/3))
- Apply cargo fmt
- *(db)* Replace per-commit rows with validity-range dedup (SCD Type 2)
- Convert to Cargo workspace monorepo
