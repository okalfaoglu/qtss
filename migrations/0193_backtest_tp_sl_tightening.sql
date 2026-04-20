-- 0193_backtest_tp_sl_tightening.sql
--
-- Faz 12.D — backtest TP/SL parameter tightening.
--
-- İlk eval koşumunda kazanma oranı %95-100 civarında çıktı (TP1 fazla
-- yakın, SL fazla uzak). Gerçekçi eğri için D noktasından sonra:
--   * TP1'i CD bacağının %61.8'ine taşı (önceki %38.2)
--   * TP2'yi CD bacağının tamamına taşı (önceki %61.8)
--   * SL tampon çarpanını 0.25 ATR'ye düşür (önceki 0.5)
--   * time_stop_legs = 3 (değişmedi; cömert ama okunur)
--
-- Yalnızca config_schema default'larını değiştiriyoruz; operatör
-- ihtiyaç duyarsa `config_value` üzerinden farklı scope için override
-- edebilir (CLAUDE.md #2).

UPDATE config_schema
   SET default_value = '0.618'::jsonb
 WHERE key = 'backtest.harmonic.tp1_retrace';

UPDATE config_schema
   SET default_value = '1.000'::jsonb
 WHERE key = 'backtest.harmonic.tp2_retrace';

UPDATE config_schema
   SET default_value = '0.25'::jsonb
 WHERE key = 'backtest.harmonic.sl_buffer_atr';
