// eve-dogma-j CLI: stateless FitRequest JSON in -> FitStats JSON out (contract v1).
#include <poll.h>
#include <sys/stat.h>
#include <unistd.h>

#include <atomic>
#include <chrono>
#include <cstdio>
#include <cstdlib>
#include <cstring>
#include <memory>
#include <simdjson.h>
#include <string>
#include <thread>
#include <vector>

#include "api.hpp"

using namespace evej;

static const char* USAGE =
    "eve-dogma-j <command> [--dataset PATH] [--cache PATH | --no-cache] [--threads N] [args]\n\n"
    "Commands:\n"
    "  calc [FILE]            FitRequest JSON (file or stdin) -> FitStats JSON\n"
    "  batch                  JSONL FitRequests on stdin -> JSONL FitStats on stdout (same order, multi-threaded)\n"
    "  serve-stdio            JSONL RPC: {\"id\",\"method\":\"calc|search|type|meta\",\"params\"}\n"
    "  search QUERY | type ID|NAME | meta\n"
    "  bench [FILE] [-n N]    time N calculations of one request in-process\n"
    "  build-cache            (re)build the binary dataset cache and exit\n\n"
    "Dataset: --dataset PATH, or $EVE_DOGMA_DATASET, or ./dataset.json.gz\n"
    "Cache:   --cache PATH, or $EVE_DOGMA_J_CACHE, or $XDG_CACHE_HOME|~/.cache/eve-dogma-j/<name>-<hash>.bin\n";

static double now_ms() {
  return std::chrono::duration<double, std::milli>(std::chrono::steady_clock::now().time_since_epoch()).count();
}

static std::string default_cache(const std::string& ds_path) {
  if (const char* e = getenv("EVE_DOGMA_J_CACHE")) return e;
  char* rp = realpath(ds_path.c_str(), nullptr);
  std::string abs = rp ? rp : ds_path;
  free(rp);
  uint64_t h = 1469598103934665603ull;
  for (unsigned char c : abs) h = (h ^ c) * 1099511628211ull;
  std::string base;
  if (const char* x = getenv("XDG_CACHE_HOME"); x && *x) base = x;
  else if (const char* hm = getenv("HOME"); hm && *hm) base = std::string(hm) + "/.cache";
  else base = "/tmp";
  mkdir(base.c_str(), 0755);
  base += "/eve-dogma-j";
  mkdir(base.c_str(), 0755);
  size_t sl = abs.find_last_of('/');
  std::string name = sl == std::string::npos ? abs : abs.substr(sl + 1);
  char hx[24];
  snprintf(hx, sizeof hx, "%016llx", (unsigned long long)h);
  return base + "/" + name + "-" + hx + ".bin";
}

static bool read_all(FILE* f, std::string& out) {
  char buf[1 << 16];
  size_t n;
  while ((n = fread(buf, 1, sizeof buf, f)) > 0) out.append(buf, n);
  return true;
}

static void write_out(const std::string& s) { fwrite(s.data(), 1, s.size(), stdout); }

static bool stdin_ready() {
  pollfd p{0, POLLIN, 0};
  return poll(&p, 1, 0) > 0;
}

// Batch: read what is available, compute in parallel, emit in order; flush whenever input pauses.
static int run_batch(const Dataset& ds, const Ids& ids, int threads) {
  std::vector<std::unique_ptr<Worker>> workers;
  for (int i = 0; i < threads; i++) workers.push_back(std::make_unique<Worker>(ds, ids));
  std::string pending;
  std::vector<char> buf(1 << 20);
  bool eof = false;
  std::vector<std::string_view> lines;
  std::vector<std::string> outs;
  while (!eof) {
    // read at least once (blocking), then keep reading while more data is immediately available
    do {
      ssize_t n = read(0, buf.data(), buf.size());
      if (n <= 0) {
        eof = true;
        break;
      }
      pending.append(buf.data(), (size_t)n);
    } while (stdin_ready() && pending.size() < (64u << 20));
    // split complete lines
    lines.clear();
    size_t start = 0, consumed = 0;
    for (size_t i = 0; i < pending.size(); i++)
      if (pending[i] == '\n') {
        lines.emplace_back(pending.data() + start, i - start);
        start = consumed = i + 1;
      }
    if (eof && start < pending.size()) {
      lines.emplace_back(pending.data() + start, pending.size() - start);
      consumed = pending.size();
    }
    // drop blank lines
    std::vector<std::string_view> work;
    work.reserve(lines.size());
    for (auto l : lines) {
      bool blank = true;
      for (char c : l)
        if (!isspace((unsigned char)c)) blank = false;
      if (!blank) work.push_back(l);
    }
    outs.assign(work.size(), std::string());
    int nt = (int)std::min<size_t>((size_t)threads, work.size());
    if (nt <= 1) {
      for (size_t i = 0; i < work.size(); i++) {
        workers[0]->calc_json(work[i]);
        outs[i].swap(workers[0]->out.s);
      }
    } else {
      std::atomic<size_t> next{0};
      auto job = [&](int w) {
        Worker& wk = *workers[w];
        for (size_t i; (i = next.fetch_add(1, std::memory_order_relaxed)) < work.size();) {
          wk.calc_json(work[i]);
          outs[i].swap(wk.out.s);
        }
      };
      std::vector<std::thread> th;
      for (int w = 1; w < nt; w++) th.emplace_back(job, w);
      job(0);
      for (auto& t : th) t.join();
    }
    for (auto& o : outs) {
      o.push_back('\n');
      write_out(o);
    }
    pending.erase(0, consumed);
    if (!stdin_ready()) fflush(stdout);
  }
  fflush(stdout);
  return 0;
}

static void rpc_line(Worker& wk, std::string_view line, JW& w) {
  w.clear();
  simdjson::dom::element root;
  if (wk.parser->parse(line.data(), line.size()).get(root)) {
    w.obj().key("error").obj().ks("code", "BAD_JSON").ks("message", "invalid JSON").end_obj().knull("id").end_obj();
    return;
  }
  simdjson::dom::element id, params;
  bool has_id = root["id"].get(id) == simdjson::SUCCESS;
  bool has_p = root["params"].get(params) == simdjson::SUCCESS;
  std::string_view method = "calc";
  simdjson::dom::element m;
  if (root["method"].get(m) == simdjson::SUCCESS) {
    std::string_view s;
    if (m.get_string().get(s) == simdjson::SUCCESS) method = s;
  }
  w.obj().key("id");
  if (has_id) w.raw(simdjson::minify(id));
  else w.null();
  w.key("result");
  auto pstr = [&](const char* k) -> std::string_view {
    std::string_view s;
    if (has_p && params[k].get_string().get(s) == simdjson::SUCCESS) return s;
    return {};
  };
  if (method == "calc") {
    if (!has_p) write_error(w, "BAD_REQUEST", "missing params", "");
    else wk.calc_element(params, w);
  } else if (method == "meta") {
    meta_json(wk.ds, w);
  } else if (method == "search") {
    uint64_t lim = 20;
    if (has_p) {
      uint64_t l;
      if (params["limit"].get_uint64().get(l) == simdjson::SUCCESS) lim = l;
    }
    search_json(wk.ds, pstr("query"), lim, w);
  } else if (method == "type") {
    std::string key;
    simdjson::dom::element x;
    if (has_p && params["id"].get(x) == simdjson::SUCCESS) {
      std::string_view s;
      if (x.get_string().get(s) == simdjson::SUCCESS) key = s;
      else key = simdjson::minify(x);
    }
    type_json(wk.ds, key, w);
  } else {
    w.obj().key("error").obj().ks("code", "UNKNOWN_METHOD").ks("message", method).end_obj().end_obj();
  }
  w.end_obj();
}

int main(int argc, char** argv) {
  std::vector<std::string> args(argv + 1, argv + argc);
  std::string dataset, cache;
  bool use_cache = true;
  int threads = (int)std::max(1u, std::thread::hardware_concurrency());
  long bench_n = 1000;
  auto take = [&](const char* flag, std::string& out) {
    for (size_t i = 0; i < args.size(); i++)
      if (args[i] == flag) {
        if (i + 1 < args.size()) out = args[i + 1];
        args.erase(args.begin() + i, args.begin() + std::min(args.size(), i + 2));
        return true;
      }
    return false;
  };
  std::string tmp;
  take("--dataset", dataset);
  take("--cache", cache);
  if (take("--threads", tmp)) threads = std::max(1, atoi(tmp.c_str()));
  if (take("-n", tmp)) bench_n = std::max(1L, atol(tmp.c_str()));
  for (size_t i = 0; i < args.size(); i++)
    if (args[i] == "--no-cache") {
      use_cache = false;
      args.erase(args.begin() + i);
      break;
    }
  std::string cmd = args.empty() ? "" : args[0];
  if (cmd.empty() || cmd == "help" || cmd == "--help" || cmd == "-h") {
    fputs(USAGE, stderr);
    return 0;
  }
  if (dataset.empty()) {
    const char* e = getenv("EVE_DOGMA_DATASET");
    dataset = e ? e : "dataset.json.gz";
  }
  static const char* known[] = {"calc", "batch", "serve-stdio", "search", "type", "meta", "bench", "build-cache"};
  bool ok_cmd = false;
  for (auto k : known)
    if (cmd == k) ok_cmd = true;
  if (!ok_cmd) {
    fputs(USAGE, stderr);
    return 2;
  }
  if (use_cache && cache.empty()) cache = default_cache(dataset);
  if (cmd == "build-cache") unlink(cache.c_str());
  double t0 = now_ms();
  std::string err;
  std::unique_ptr<Dataset> ds(Dataset::open(dataset, cache, use_cache, err));
  if (!ds) {
    fprintf(stderr, "error: %s\n", err.c_str());
    return 3;
  }
  Ids ids(*ds);
  double load_ms = now_ms() - t0;
  static char obuf[1 << 16];
  setvbuf(stdout, obuf, _IOFBF, sizeof obuf);

  if (cmd == "build-cache") {
    fprintf(stderr, "cache: %s (%.1f ms)\n", cache.c_str(), load_ms);
    return 0;
  }
  if (cmd == "calc" || cmd == "bench") {
    std::string in;
    if (args.size() > 1 && args[1] != "-") {
      FILE* f = fopen(args[1].c_str(), "rb");
      if (!f) {
        fprintf(stderr, "error: %s: cannot open\n", args[1].c_str());
        return 2;
      }
      read_all(f, in);
      fclose(f);
    } else {
      read_all(stdin, in);
    }
    Worker wk(*ds, ids);
    if (cmd == "calc") {
      bool ok = wk.calc_json(in);
      wk.out.s.push_back('\n');
      write_out(wk.out.s);
      fflush(stdout);
      return ok ? 0 : 2;
    }
    wk.calc_json(in);
    double t1 = now_ms();
    size_t sink = 0;
    for (long i = 0; i < bench_n; i++) {
      wk.calc_json(in);
      sink += wk.out.s.size();
    }
    double el = (now_ms() - t1) / 1000.0;
    JW w;
    w.obj().key("dataset_load_ms").num_raw(load_ms).ki("iterations", bench_n).key("per_calc_us").num_raw(el / bench_n * 1e6)
        .key("total_s").num_raw(el).ki("output_bytes", (int64_t)(sink / bench_n)).end_obj();
    w.s.push_back('\n');
    write_out(w.s);
    return 0;
  }
  if (cmd == "batch") return run_batch(*ds, ids, threads);
  if (cmd == "serve-stdio") {
    fprintf(stderr, "eve-dogma-j serve-stdio ready (sde %llu)\n", (unsigned long long)ds->build);
    Worker wk(*ds, ids);
    JW w;
    std::string line;
    char* lp = nullptr;
    size_t cap = 0;
    ssize_t n;
    while ((n = getline(&lp, &cap, stdin)) > 0) {
      std::string_view l(lp, (size_t)n);
      while (!l.empty() && (l.back() == '\n' || l.back() == '\r')) l.remove_suffix(1);
      bool blank = true;
      for (char c : l)
        if (!isspace((unsigned char)c)) blank = false;
      if (blank) continue;
      rpc_line(wk, l, w);
      w.s.push_back('\n');
      write_out(w.s);
      fflush(stdout);
    }
    free(lp);
    return 0;
  }
  JW w;
  std::string rest;
  for (size_t i = 1; i < args.size(); i++) rest += (i > 1 ? " " : "") + args[i];
  if (cmd == "meta") meta_json(*ds, w, load_ms);
  else if (cmd == "type") type_json(*ds, rest, w);
  else if (cmd == "search") search_json(*ds, rest, 25, w);
  w.s.push_back('\n');
  write_out(w.s);
  return 0;
}
