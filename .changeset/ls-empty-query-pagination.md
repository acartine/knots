---
"knots": patch
---

`kno ls --json` now reports consistent pagination metadata for text queries
that filter out every row on the requested page. Previously, the SQL layer
paginated before the in-memory query filter ran, so a query with no matches
could return `data: []` alongside `total: 50` and `has_more: true`. The CLI
now applies all filters before paginating, so empty matches report
`total: 0` and `has_more: false`.
