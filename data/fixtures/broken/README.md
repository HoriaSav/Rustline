# broken — fixture feed with known problems

Invented feed (timezone Pacific/Auckland) used to test the validation report
and frequencies.txt expansion. Every trip except `freq` is wrong on purpose.

| trip | problem |
|------|---------|
| `one` | only 1 stop time |
| `back` | time goes backwards |
| `ghost` | unknown service_id |
| `lost` | unknown route_id |
| `edge` | no time at the first stop |
| `freq` | ok: 06–07 every 20 min (headway only), 07–08 every 30 min (exact), plus one invalid row (headway 0) |

Stop `Z` has no coordinates.
