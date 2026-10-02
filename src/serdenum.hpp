#pragma once
#include <string>
#include <string_view>

namespace evej {
// serde_json (without float_roundtrip, as eve-dogma-rs builds it) does not always round decimal numbers
// correctly: f64 = significand (first 19 digits) as f64, then one multiply/divide by a power of ten. For each JSON
// number whose serde value differs from the correctly rounded one (only possible with >= 16 significant digits or
// a large exponent), and for "-0" (serde: float -0.0), rewrite the token as the shortest repr of the serde value
// (always with '.' or 'e', so it stays a float), so simdjson's exact parsing yields serde's double.
// Returns false (and leaves `out` untouched) when nothing needs rewriting, which is the common case.
bool serde_fix_numbers(std::string_view in, std::string& out);
}  // namespace evej
