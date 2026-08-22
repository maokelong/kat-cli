---
status: accepted
---

# PostgreSQL common 首版支持最小封闭类型集合

公共 PostgreSQL common 第一版根据 `cursor.description` 中的 PostgreSQL OID 和可用类型修饰信息构造以下 PyArrow Schema；NULL 保留为相应 nullable 列中的 NULL，零行 rowset 仍使用相同映射构造 Schema：

| PostgreSQL | PyArrow |
| --- | --- |
| `boolean` | `bool` |
| `smallint` | `int16` |
| `integer` | `int32` |
| `bigint` | `int64` |
| `real` | `float32` |
| `double precision` | `float64` |
| `numeric(p,s)` 且 `p <= 38` | `decimal128(p,s)` |
| `name`、`text`、`varchar`、`char` | `string` |
| `bytea` | `binary` |
| `date` | `date32` |
| `time` | `time64[us]` |
| `timestamp` | `timestamp[us]` |
| `timestamp with time zone` | `timestamp[us, UTC]` |

`numeric` 只接受 description 同时提供 precision/scale、`1 <= precision <= 38` 且 `0 <= scale <= precision` 的普通有限值；无约束或类型修饰丢失的 numeric、numeric NaN/Infinity、UUID、JSON/JSONB、数组、复合类型、枚举、区间、网络地址和扩展类型第一版明确失败。调用 SQL 可以显式 cast 为受支持类型，例如 `numeric(38, 2)` 或 `text`。PostgreSQL 10 及以上会在输出边界把裸 `NULL` 解析为 `text`；Psycopg 的列描述与显式 `NULL::text` 相同，在“不解析 SQL”的合同下 common 因此按服务端返回的 text OID 接受并保留该 NULL。common 不按样本值猜测列类型，不把不支持的值静默转换为 string。该决定细化 ADR-0075 的封闭映射合同。
