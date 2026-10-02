// Minimal streaming JSON writer. Number formatting matches serde_json (ryu shortest, "1.0", "1e-7").
// Callers emit object keys in sorted (BTreeMap) order so output is byte-comparable with eve-dogma-rs.
#pragma once
#include <charconv>
#include <cmath>
#include <cstdint>
#include <cstring>
#include <string>
#include <string_view>

namespace evej {

inline double round6(double v) { return std::isfinite(v) ? std::round(v * 1e6) / 1e6 : v; }

class JW {
 public:
  std::string s;
  JW() { s.reserve(8192); }
  void clear() { s.clear(); first_ = true; }

  JW& obj() { sep(); s.push_back('{'); first_ = true; return *this; }
  JW& end_obj() { s.push_back('}'); first_ = false; return *this; }
  JW& arr() { sep(); s.push_back('['); first_ = true; return *this; }
  JW& end_arr() { s.push_back(']'); first_ = false; return *this; }
  JW& key(std::string_view k) {
    sep();
    str_raw(k);
    s.push_back(':');
    first_ = true;  // value follows without comma
    return *this;
  }
  JW& null() { sep(); s.append("null"); return *this; }
  JW& boolean(bool b) { sep(); s.append(b ? "true" : "false"); return *this; }
  JW& i64(int64_t v) {
    sep();
    char b[24];
    auto r = std::to_chars(b, b + 24, v);
    s.append(b, r.ptr);
    return *this;
  }
  // f64 value (rounded to 6 decimals like eve-dogma-rs's tidy())
  JW& num(double v) { sep(); f64_raw(round6(v)); return *this; }
  JW& num_raw(double v) { sep(); f64_raw(v); return *this; }
  JW& str(std::string_view v) { sep(); str_raw(v); return *this; }
  // pre-serialised JSON value
  JW& raw(std::string_view v) { sep(); s.append(v); return *this; }
  // helpers
  JW& kn(std::string_view k, double v) { return key(k).num(v); }
  JW& ki(std::string_view k, int64_t v) { return key(k).i64(v); }
  JW& ks(std::string_view k, std::string_view v) { return key(k).str(v); }
  JW& kb(std::string_view k, bool v) { return key(k).boolean(v); }
  JW& knull(std::string_view k) { return key(k).null(); }

  void f64_raw(double v) {
    if (!std::isfinite(v)) {
      s.append("null");
      return;
    }
    char buf[64];
    // shortest round-trip digits in scientific form: d[.ddd]e±XX
    auto r = std::to_chars(buf, buf + 64, v, std::chars_format::scientific);
    std::string_view sc(buf, r.ptr - buf);
    bool neg = false;
    if (sc[0] == '-') {
      neg = true;
      sc.remove_prefix(1);
    }
    size_t epos = sc.find('e');
    std::string_view mant = sc.substr(0, epos);
    int exp10 = 0;
    std::from_chars(sc.data() + epos + 1 + (sc[epos + 1] == '+' ? 1 : 0), sc.data() + sc.size(), exp10);
    char digits[32];
    int nd = 0;
    for (char c : mant)
      if (c != '.') digits[nd++] = c;
    // strip trailing zeros (to_chars gives shortest, but "0e+00" for zero)
    while (nd > 1 && digits[nd - 1] == '0') nd--;
    if (neg) s.push_back('-');
    if (nd == 1 && digits[0] == '0') {
      s.append("0.0");
      return;
    }
    int kk = exp10 + 1;  // position of decimal point relative to digit string
    if (kk > 0 && kk <= 16) {
      if (nd <= kk) {
        s.append(digits, nd);
        s.append((size_t)(kk - nd), '0');
        s.append(".0");
      } else {
        s.append(digits, kk);
        s.push_back('.');
        s.append(digits + kk, nd - kk);
      }
    } else if (kk <= 0 && kk > -5) {
      s.append("0.");
      s.append((size_t)(-kk), '0');
      s.append(digits, nd);
    } else {
      s.push_back(digits[0]);
      if (nd > 1) {
        s.push_back('.');
        s.append(digits + 1, nd - 1);
      }
      s.push_back('e');
      char eb[8];
      auto er = std::to_chars(eb, eb + 8, kk - 1);
      s.append(eb, er.ptr);
    }
  }

 private:
  bool first_ = true;
  void sep() {
    if (!first_) s.push_back(',');
    first_ = false;
  }
  void str_raw(std::string_view v) {
    s.push_back('"');
    for (unsigned char c : v) {
      switch (c) {
        case '"': s.append("\\\""); break;
        case '\\': s.append("\\\\"); break;
        case '\n': s.append("\\n"); break;
        case '\r': s.append("\\r"); break;
        case '\t': s.append("\\t"); break;
        case '\b': s.append("\\b"); break;
        case '\f': s.append("\\f"); break;
        default:
          if (c < 0x20) {
            static const char* hx = "0123456789abcdef";
            s.append("\\u00");
            s.push_back(hx[c >> 4]);
            s.push_back(hx[c & 15]);
          } else {
            s.push_back((char)c);
          }
      }
    }
    s.push_back('"');
  }
};

}  // namespace evej
