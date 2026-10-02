// WebAssembly / browser entry points (Emscripten build, see CMakeLists.txt and wasm/). The host writes the dataset
// (.json.gz) into the Emscripten file system, calls evej_open once, then evej_calc / evej_rpc per request. Returned
// strings stay valid until the next call.
#include <emscripten/emscripten.h>

#include <memory>
#include <string>

#include "api.hpp"

namespace {
std::unique_ptr<evej::Dataset> g_ds;
std::unique_ptr<evej::Ids> g_ids;
std::unique_ptr<evej::Worker> g_wk;
std::string g_err;
evej::JW g_rpc;
}  // namespace

extern "C" {
// Load the dataset at `path` (in the Emscripten FS). Returns "" on success, else an error message.
EMSCRIPTEN_KEEPALIVE const char* evej_open(const char* path) {
  g_wk.reset();
  g_ids.reset();
  g_err.clear();
  g_ds.reset(evej::Dataset::open(path, std::string(), false, g_err));
  if (!g_ds) return g_err.c_str();
  g_ids = std::make_unique<evej::Ids>(*g_ds);
  g_wk = std::make_unique<evej::Worker>(*g_ds, *g_ids);
  return "";
}
// FitRequest JSON -> FitStats JSON (or an {"error":...} object), as the `calc` CLI command.
EMSCRIPTEN_KEEPALIVE const char* evej_calc(const char* request) {
  if (!g_wk) return R"({"error":{"code":"NOT_LOADED","message":"call evej_open first","path":""}})";
  g_wk->calc_json(request);
  return g_wk->out.s.c_str();
}
// One serve-stdio line {"id","method","params"} -> response line.
EMSCRIPTEN_KEEPALIVE const char* evej_rpc(const char* line) {
  if (!g_wk) return R"({"error":{"code":"NOT_LOADED","message":"call evej_open first"},"id":null})";
  evej::rpc_line(*g_wk, line, g_rpc);
  return g_rpc.s.c_str();
}
}
