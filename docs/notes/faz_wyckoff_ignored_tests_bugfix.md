# BUGFIX — Wyckoff ignored test suite rebuild

Durum: **backlog** (backtest GUI + scallop kalibrasyonu sonrası).

## Kapsam
Aşağıdaki 4 test `#[ignore]` ile işaretli, pre-existing regression olarak
takip ediliyor. Phase-C gating + Detection API değişikliklerinden sonra
fixture ve beklentiler güncellenmedi.

| Crate / Modül | Test | Root cause notu |
|---|---|---|
| `qtss-wyckoff` setup_builder | `test::lpsy_emits_short_setup` | Phase progression gating sonrası LPSY short setup emit etmiyor — gate koşulu gevşetilecek veya fixture Phase-D'ye taşınacak |
| `qtss-wyckoff` structure | `tests::mid_structure_event_promotes_phase` | Mid-structure event phase promote etmiyor — auto_reclassify hysteresis event'i yutuyor olabilir |
| `qtss-wyckoff` | `tests::detect_spring` | `Vec<Detection>` API değişikliği + daha sıkı Phase-C kapısı; fixture yeniden kurulmalı |
| `qtss-wyckoff` | `tests::detect_upthrust` | `detect_spring` ile aynı — bear tarafı |

## İş Adımları
1. Her test için ignore attribute'unu kaldır, failure mesajını oku.
2. Fixture'ları Phase-C / Phase-D'ye taşıyan helper yaz (AR + ST + test
   event dizisi); mevcut manuel bar setup'lardan gerekirse `StructureState`
   builder'ına geç.
3. Phase-C gating koşullarını `structure.rs`'te incele; test kapsamı için
   gerekiyorsa `config`-driven bir `phase_c_required_for_spring` bayrağı
   ekle (CLAUDE.md #2 — hardcoded yok).
4. 4 test de yeşil olduktan sonra `cargo test -p qtss-wyckoff` tam yeşil.

## Öncelik
Backtest GUI + scallop kalibrasyonu bittikten sonra.
