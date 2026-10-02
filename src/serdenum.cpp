#include "serdenum.hpp"

#include <charconv>
#include <cmath>
#include <cstdint>
#include <cstring>
#include <optional>

namespace evej {
namespace {
const double* pow10_table() {
  static double t[309];
  static bool init = [] {
    // exactly Rust's f64 literals 1e0..1e308 (correctly rounded)
    for (int i = 0; i <= 308; i++) {
      char b[8];
      snprintf(b, sizeof b, "1e%d", i);
      std::from_chars(b, b + strlen(b), t[i]);
    }
    return true;
  }();
  (void)init;
  return t;
}
inline bool ovf64(uint64_t a, uint64_t b) {
  const uint64_t c = UINT64_MAX;
  return a >= c / 10 && (a > c / 10 || b > c % 10);
}
inline bool ovf32(int32_t a, int32_t b) {
  const int32_t c = INT32_MAX;
  return a >= c / 10 && (a > c / 10 || b > c % 10);
}
// serde_json's f64_from_parts (no float_roundtrip); nullopt = NumberOutOfRange
std::optional<double> from_parts(bool positive, uint64_t significand, int32_t exponent) {
  const double* P = pow10_table();
  double f = (double)significand;
  while (true) {
    uint32_t ab = exponent < 0 ? (uint32_t)0 - (uint32_t)exponent : (uint32_t)exponent;  // wrapping_abs
    if (ab <= 308) {
      if (exponent >= 0) {
        f *= P[ab];
        if (std::isinf(f)) return std::nullopt;
      } else {
        f /= P[ab];
      }
      break;
    }
    if (f == 0.0) break;
    if (exponent >= 0) return std::nullopt;
    f /= 1e308;
    exponent += 308;
  }
  return positive ? f : -f;
}
// serde_json's number grammar and value for a float token; nullopt = not a valid number or out of range
// is_float: whether serde yields an f64 (vs u64/i64)
std::optional<double> serde_value(std::string_view s, bool& is_float) {
  size_t i = 0, n = s.size();
  bool positive = true;
  is_float = false;
  if (i < n && s[i] == '-') {
    positive = false;
    i++;
  }
  if (i >= n) return std::nullopt;
  uint64_t sig = 0;
  int32_t exp_before = 0;
  bool long_int = false;
  if (s[i] == '0') {
    i++;
    if (i < n && s[i] >= '0' && s[i] <= '9') return std::nullopt;
  } else if (s[i] >= '1' && s[i] <= '9') {
    while (i < n && s[i] >= '0' && s[i] <= '9') {
      uint64_t d = (uint64_t)(s[i] - '0');
      if (!long_int && ovf64(sig, d)) long_int = true;
      if (long_int) exp_before++;
      else sig = sig * 10 + d;
      i++;
    }
  } else {
    return std::nullopt;
  }
  int32_t exponent = exp_before;
  if (i < n && s[i] == '.') {
    is_float = true;
    i++;
    int32_t after = 0;
    bool any = false, over = false;
    while (i < n && s[i] >= '0' && s[i] <= '9') {
      any = true;
      uint64_t d = (uint64_t)(s[i] - '0');
      if (!over && ovf64(sig, d)) over = true;
      if (!over) {
        sig = sig * 10 + d;
        after--;
      }
      i++;
    }
    if (!any) return std::nullopt;
    exponent = exp_before + after;
  }
  if (i < n && (s[i] == 'e' || s[i] == 'E')) {
    is_float = true;
    i++;
    bool pos_exp = true;
    if (i < n && (s[i] == '+' || s[i] == '-')) {
      pos_exp = s[i] == '+';
      i++;
    }
    if (i >= n || s[i] < '0' || s[i] > '9') return std::nullopt;
    int32_t e = s[i] - '0';
    i++;
    bool eo = false;
    while (i < n && s[i] >= '0' && s[i] <= '9') {
      int32_t d = s[i] - '0';
      if (!eo && ovf32(e, d)) eo = true;
      if (!eo) e = e * 10 + d;
      i++;
    }
    if (i != n) return std::nullopt;
    if (eo) {
      if (sig != 0 && pos_exp) return std::nullopt;
      return positive ? 0.0 : -0.0;
    }
    int64_t fe = pos_exp ? (int64_t)exponent + e : (int64_t)exponent - e;  // saturating
    if (fe > INT32_MAX) fe = INT32_MAX;
    if (fe < INT32_MIN) fe = INT32_MIN;
    return from_parts(positive, sig, (int32_t)fe);
  }
  if (i != n) return std::nullopt;
  if (is_float || long_int) {
    is_float = true;
    return from_parts(positive, sig, exponent);
  }
  if (positive) return (double)sig;
  // negative integer: i64 unless it underflows (or is -0): then f64 -(sig as f64)
  int64_t neg = (int64_t)(0 - sig);
  if (neg >= 0) {
    is_float = true;
    return -(double)sig;
  }
  return (double)neg;
}
inline bool numch(char c) { return (c >= '0' && c <= '9') || c == '.' || c == 'e' || c == 'E' || c == '+' || c == '-'; }
}  // namespace

bool serde_fix_numbers(std::string_view in, std::string& out) {
  const char* p = in.data();
  const size_t n = in.size();
  std::string res;
  size_t copied = 0;
  bool changed = false;
  size_t i = 0;
  while (i < n) {
    char c = p[i];
    if (c == '"') {
      // skip the string (escapes)
      i++;
      while (i < n) {
        const void* q = memchr(p + i, '"', n - i);
        if (!q) {
          i = n;
          break;
        }
        size_t j = (size_t)((const char*)q - p);
        size_t bs = 0;
        while (j - bs > i && p[j - bs - 1] == '\\') bs++;
        i = j + 1;
        if (bs % 2 == 0) break;
      }
      continue;
    }
    if (c == '-' || (c >= '0' && c <= '9')) {
      size_t j = i;
      while (j < n && numch(p[j])) j++;
      std::string_view tok(p + i, j - i);
      bool hard = tok.size() >= 16 || tok == "-0";
      if (!hard)
        for (char t : tok)
          if (t == 'e' || t == 'E') hard = true;
      if (hard) {
        bool is_float;
        std::optional<double> sv = serde_value(tok, is_float);
        if (sv && is_float) {
          double cv;
          auto r = std::from_chars(tok.data(), tok.data() + tok.size(), cv);
          uint64_t a, b;
          memcpy(&a, &*sv, 8);
          memcpy(&b, &cv, 8);
          bool simd_float = tok.find_first_of(".eE") != std::string_view::npos;
          if (r.ec != std::errc() || r.ptr != tok.data() + tok.size() || a != b || !simd_float) {
            char buf[40];
            auto w = std::to_chars(buf, buf + 32, *sv);
            std::string_view rep(buf, (size_t)(w.ptr - buf));
            res.append(p + copied, i - copied);
            res.append(rep);
            if (rep.find_first_of(".e") == std::string_view::npos) res.append(".0");
            copied = j;
            changed = true;
          }
        }
      }
      i = j;
      continue;
    }
    i++;
  }
  if (!changed) return false;
  res.append(p + copied, n - copied);
  out = std::move(res);
  return true;
}
}  // namespace evej
