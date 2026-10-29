# Independent parser patch review

Reviewer integration_transport_review found and closed two Important and one Minor:
std cap accidentally changed no_std const-generic dispatch; exact-byte OSC52 filter
allowed numeric equivalents through a generic consumer; initial decimal conversion
mistook4< for52. Actual RED/GREEN preserved upstream no_std behavior and now validates
ASCII digits with checked arithmetic.52/052/0052 blocked;53/4< forwarded.

Independent source review checked consumed-prefix/overflow/degrade, raw BEL/split-ST
discard, C0/separator budgets and MaybeUninit bounds (<=16 initialized parameter
boundaries). Current errors/logs carry no payload. No open finding blocks this local
checkpoint; full E04/P02 remains OPEN as listed in README.

Author verified locked terminal8/8, vendor all-features54/54, no_std36/36+doc1/1,
strict Clippy for each, official-source apply-check and replay identical lib content.
Final patch SHA256004337c6923948880d2adff76cf54a85d33eb43c8f531057a815792a4787dfd0;
vendor lib SHA2568dff815e49a29eed18f70cb9e67a9a25d981d7247c5f7333f499fa1450deac07.
Earlier fmt-all nightly-option warnings did not modify vendor source, independently
rechecked. Root uses package-scoped formatting to preserve vendored provenance.
