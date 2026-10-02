#pragma once
#include <memory>
#include <string>
#include <string_view>
#include <vector>

#include "dataset.hpp"
#include "engine.hpp"
#include "jsonw.hpp"

namespace simdjson::dom { class parser; class element; }

namespace evej {
// Per-thread reusable state (parser + output buffer).
struct Worker {
  explicit Worker(const Dataset& ds, const Ids& ids);
  ~Worker();
  const Dataset& ds;
  const Ids& ids;
  simdjson::dom::parser* parser;
  JW out;
  std::unique_ptr<Fit> fit;  // reused across requests (capacity kept)
  // FitRequest JSON text -> FitStats JSON (in out.s). Returns false if the response is an error object.
  bool calc_json(std::string_view request);
  bool calc_element(const simdjson::dom::element& e, JW& w);
};
// One serve-stdio line {"id","method","params"} -> response object in w (methods: calc, eft_parse, eft_export,
// meta, search, type; anything else -> UNKNOWN_METHOD).
void rpc_line(Worker& wk, std::string_view line, JW& w);
void write_error(JW& w, const char* code, std::string_view message, std::string_view path);
void meta_json(const Dataset& ds, JW& w, double load_ms = -1);
void type_json(const Dataset& ds, std::string_view key, JW& w);
void search_json(const Dataset& ds, std::string_view q, size_t limit, JW& w,
                 const std::vector<std::string>* kinds = nullptr);
}  // namespace evej
