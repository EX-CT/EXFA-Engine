#include <type_traits>
// eve-dogma-j CLI: stateless FitRequest JSON in -> FitStats JSON out (contract v1).
#include <poll.h>
#include <sys/stat.h>
#include <unistd.h>

#include <atomic>
#include <condition_variable>
#include <deque>
#include <mutex>
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
#include "eft.hpp"
#include "request.hpp"

using namespace evej;

static const char* USAGE =
    "eve-dogma-j <command> [--dataset PATH] [--cache PATH | --no-cache] [--threads N] [args]\n\n"
    "Commands:\n"
    "  calc [FILE]            FitRequest JSON (file or stdin) -> FitStats JSON\n"
    "  batch                  JSONL FitRequests on stdin -> JSONL FitStats on stdout (same order, multi-threaded)\n"
    "  serve-stdio            JSONL RPC: {\"id\",\"method\":\"calc|eft_parse|eft_export|search|type|meta\",\"params\"}\n"
    "  eft [FILE]             EFT text (file or stdin) -> FitRequest JSON (add --calc to compute, --skills N)\n"
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
  return base + "/" + name + "-" + hx + "-v" + std::to_string(CACHE_VERSION) + ".bin";
}

static bool read_all(FILE* f, std::string& out) {
  char buf[1 << 16];
  size_t n;
  while ((n = fread(buf, 1, sizeof buf, f)) > 0) out.append(buf, n);
  return true;
}

// Resolved attribute/effect ids are cached next to the dataset image (<cache>.ids), keyed by the dataset sha256.
static_assert(std::is_trivially_copyable_v<Ids>);
// Also keyed by the executable's size+mtime, so a rebuilt engine never reuses ids resolved by another build.
struct IdsFile {
  char magic[8];
  uint32_t size, version;
  char sha[64];
  int64_t exe_size, exe_mtime_ns;
  Ids ids;
};
static void exe_id(int64_t& size, int64_t& mt) {
  struct stat st {};
  size = mt = -1;
  if (stat("/proc/self/exe", &st) == 0) {
    size = (int64_t)st.st_size;
    mt = (int64_t)st.st_mtim.tv_sec * 1000000000ll + st.st_mtim.tv_nsec;
  }
}
static Ids load_ids(const Dataset& ds, const std::string& cache) {
  int64_t es, em;
  exe_id(es, em);
  if (!cache.empty() && es >= 0) {
    std::string p = cache + ".ids";
    if (FILE* f = fopen(p.c_str(), "rb")) {
      IdsFile x;
      bool ok = fread(&x, 1, sizeof x, f) == sizeof x;
      fclose(f);
      if (ok && memcmp(x.magic, "EVEJIDS1", 8) == 0 && x.size == sizeof(Ids) && x.version == CACHE_VERSION &&
          ds.sha256.size() == 64 && memcmp(x.sha, ds.sha256.data(), 64) == 0 && x.exe_size == es &&
          x.exe_mtime_ns == em)
        return x.ids;
    }
  }
  Ids ids(ds);
  if (!cache.empty() && ds.sha256.size() == 64 && es >= 0) {
    IdsFile x{};
    x.exe_size = es;
    x.exe_mtime_ns = em;
    memcpy(x.magic, "EVEJIDS1", 8);
    x.size = sizeof(Ids);
    x.version = CACHE_VERSION;
    memcpy(x.sha, ds.sha256.data(), 64);
    x.ids = ids;
    std::string p = cache + ".ids", tmp = p + ".tmp." + std::to_string(getpid());
    if (FILE* f = fopen(tmp.c_str(), "wb")) {
      bool ok = fwrite(&x, 1, sizeof x, f) == sizeof x;
      ok = fclose(f) == 0 && ok;
      if (ok) rename(tmp.c_str(), p.c_str());
      else unlink(tmp.c_str());
    }
  }
  return ids;
}

static void write_out(const std::string& s) { fwrite(s.data(), 1, s.size(), stdout); }

static bool stdin_ready() {
  pollfd p{0, POLLIN, 0};
  return poll(&p, 1, 0) > 0;
}

// Batch: read what is available, compute in parallel, emit in order; flush whenever input pauses.
static bool is_blank(std::string_view l) {
  for (char c : l)
    if (!isspace((unsigned char)c)) return false;
  return true;
}

// Single-threaded batch: process lines as they arrive, flush whenever input pauses.
static int run_batch_serial(const Dataset& ds, const Ids& ids) {
  Worker wk(ds, ids);
  std::string pending;
  std::vector<char> buf(1 << 20);
  bool eof = false;
  while (!eof) {
    ssize_t n = read(0, buf.data(), buf.size());
    if (n <= 0) eof = true;
    else pending.append(buf.data(), (size_t)n);
    size_t start = 0;
    for (size_t i = 0; i <= pending.size(); i++) {
      bool end = i == pending.size();
      if (end && !eof) break;
      if (end || pending[i] == '\n') {
        std::string_view l(pending.data() + start, i - start);
        start = i + 1;
        if (is_blank(l)) continue;
        wk.calc_json(l);
        wk.out.s.push_back('\n');
        write_out(wk.out.s);
      }
    }
    pending.erase(0, std::min(start, pending.size()));
    if (!stdin_ready()) fflush(stdout);
  }
  fflush(stdout);
  return 0;
}

// Parallel batch: a reader (this thread) streams lines into a job queue, N workers compute, a writer thread emits
// results strictly in input order. stdout is flushed whenever the writer has caught up with the finished results,
// so request/response pipelines never stall, while bulk input streams through without per-chunk barriers.
static int run_batch(const Dataset& ds, const Ids& ids, int threads) {
  if (threads <= 1) return run_batch_serial(ds, ids);
  struct Job {
    size_t seq;
    std::string line;
  };
  std::mutex m;
  std::condition_variable cv_job, cv_res;
  std::deque<Job> jobs;
  std::deque<std::string> res;
  std::deque<char> ready;
  size_t res_base = 0, next_seq = 0;
  bool in_done = false;
  std::vector<std::thread> pool;
  for (int t = 0; t < threads; t++)
    pool.emplace_back([&] {
      Worker wk(ds, ids);
      std::unique_lock<std::mutex> lk(m);
      while (true) {
        cv_job.wait(lk, [&] { return !jobs.empty() || in_done; });
        if (jobs.empty()) break;
        Job j = std::move(jobs.front());
        jobs.pop_front();
        lk.unlock();
        wk.calc_json(j.line);
        std::string o;
        o.reserve(wk.out.s.size() + 1);
        o.append(wk.out.s).push_back('\n');
        lk.lock();
        size_t k = j.seq - res_base;
        res[k] = std::move(o);
        ready[k] = 1;
        if (k == 0) cv_res.notify_one();
      }
    });
  std::thread writer([&] {
    std::unique_lock<std::mutex> lk(m);
    while (true) {
      cv_res.wait(lk, [&] { return (!ready.empty() && ready.front()) || (in_done && res.empty()); });
      if (res.empty()) break;
      std::vector<std::string> batch;
      while (!ready.empty() && ready.front()) {
        batch.push_back(std::move(res.front()));
        res.pop_front();
        ready.pop_front();
        res_base++;
      }
      bool caught_up = ready.empty() || !ready.front();
      lk.unlock();
      for (auto& o : batch) write_out(o);
      if (caught_up) fflush(stdout);
      lk.lock();
    }
  });
  Worker inline_wk(ds, ids);
  std::string pending;
  std::vector<char> buf(1 << 20);
  bool eof = false;
  while (!eof) {
    ssize_t n = read(0, buf.data(), buf.size());
    if (n <= 0) eof = true;
    else pending.append(buf.data(), (size_t)n);
    std::vector<Job> fresh;
    size_t start = 0;
    for (size_t i = 0; i <= pending.size(); i++) {
      bool end = i == pending.size();
      if (end && !eof) break;
      if (end || pending[i] == '\n') {
        std::string_view l(pending.data() + start, i - start);
        start = i + 1;
        if (!is_blank(l)) fresh.push_back(Job{0, std::string(l)});
      }
    }
    pending.erase(0, std::min(start, pending.size()));
    if (fresh.size() == 1) {
      // lone request with nothing in flight (interactive use): answer inline, no thread hand-offs
      bool idle;
      {
        std::lock_guard<std::mutex> g(m);
        idle = res.empty();
      }
      if (idle) {
        inline_wk.calc_json(fresh[0].line);
        inline_wk.out.s.push_back('\n');
        write_out(inline_wk.out.s);
        fflush(stdout);
        continue;
      }
    }
    if (!fresh.empty()) {
      std::lock_guard<std::mutex> g(m);
      for (auto& j : fresh) {
        j.seq = next_seq++;
        res.emplace_back();
        ready.push_back(0);
        jobs.push_back(std::move(j));
      }
    }
    if (fresh.size() == 1) cv_job.notify_one();
    else if (!fresh.empty()) cv_job.notify_all();
  }
  {
    std::lock_guard<std::mutex> g(m);
    in_done = true;
  }
  cv_job.notify_all();
  for (auto& t : pool) t.join();
  cv_res.notify_all();
  writer.join();
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
  } else if (method == "eft_parse") {
    FitRequest r;
    std::string e = eft_parse(wk.ds, pstr("text"), r);
    if (!e.empty()) w.obj().key("error").obj().ks("code", "EFT_PARSE").ks("message", e).end_obj().end_obj();
    else fit_request_json(r, w);
  } else if (method == "eft_export") {
    simdjson::dom::element f;
    FitRequest r;
    std::string e;
    if (!has_p || params["fit"].get(f) != simdjson::SUCCESS || f.is_null()) e = "invalid type: null, expected struct FitRequest";
    else e = parse_request(f, r);
    if (!e.empty()) {
      w.obj().key("error").obj().ks("code", "BAD_REQUEST").ks("message", e).end_obj().end_obj();
    } else {
      std::string_view nm = "EXCT fit";
      std::string_view x;
      if (params["name"].get_string().get(x) == simdjson::SUCCESS) nm = x;
      if (!wk.fit) wk.fit = std::make_unique<Fit>(wk.ds, wk.ids);
      else wk.fit->reset();
      EngineError ferr{};
      bool built = wk.fit->build(r, ferr);
      std::string text = eft_export(wk.ds, r, nm, built ? wk.fit.get() : nullptr);
      w.obj().ks("text", text).end_obj();
    }
  } else if (method == "meta") {
    meta_json(wk.ds, w);
  } else if (method == "search") {
    uint64_t lim = 20;
    if (has_p) {
      uint64_t l;
      if (params["limit"].get_uint64().get(l) == simdjson::SUCCESS) lim = l;
    }
    std::vector<std::string> kinds;
    bool has_kinds = false;
    simdjson::dom::array ka;
    if (has_p && params["kinds"].get_array().get(ka) == simdjson::SUCCESS) {
      has_kinds = true;
      for (auto x : ka) {
        std::string_view ks;
        if (x.get_string().get(ks) == simdjson::SUCCESS) kinds.emplace_back(ks);
      }
    }
    search_json(wk.ds, pstr("query"), lim, w, has_kinds ? &kinds : nullptr);
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
  std::string eft_skills;
  bool eft_skills_set = take("--skills", eft_skills), eft_calc = false;
  for (size_t i = 1; i < args.size(); i++)
    if (args[i] == "--calc" && !args.empty() && args[0] == "eft") {
      eft_calc = true;
      args.erase(args.begin() + i);
      break;
    }
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
  static const char* known[] = {"calc", "batch", "serve-stdio", "eft", "search", "type", "meta", "bench", "build-cache"};
  bool ok_cmd = false;
  for (auto k : known)
    if (cmd == k) ok_cmd = true;
  if (!ok_cmd) {
    fputs(USAGE, stderr);
    return 2;
  }
  if (use_cache && cache.empty()) cache = default_cache(dataset);
  if (cmd == "build-cache") {
    unlink(cache.c_str());
    unlink((cache + ".ids").c_str());
  }
  double t0 = now_ms();
  std::string err;
  std::unique_ptr<Dataset> ds(Dataset::open(dataset, cache, use_cache, err));
  if (!ds) {
    fprintf(stderr, "error: %s\n", err.c_str());
    return 3;
  }
  double t1 = now_ms();
  const Ids ids = load_ids(*ds, use_cache ? cache : std::string());
  double load_ms = now_ms() - t0;
  if (getenv("EVEJ_TIMING")) fprintf(stderr, "open %.3f ms, ids %.3f ms\n", t1 - t0, now_ms() - t1);
  static char obuf[1 << 16];
  setvbuf(stdout, obuf, _IOFBF, sizeof obuf);

  if (cmd == "build-cache") {
    fprintf(stderr, "cache: %s (%.1f ms)\n", cache.c_str(), load_ms);
    return 0;
  }
  if (cmd == "calc" || cmd == "bench" || cmd == "eft") {
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
    double tw = now_ms();
    Worker wk(*ds, ids);
    if (cmd == "eft") {
      FitRequest r;
      std::string e = eft_parse(*ds, in, r);
      if (!e.empty()) {
        fprintf(stderr, "error: %s\n", e.c_str());
        return 2;
      }
      if (eft_skills_set) {
        uint32_t lv = 0;
        bool ok = !eft_skills.empty();
        for (char c : eft_skills) {
          if (c < '0' || c > '9') ok = false;
          else lv = lv * 10 + (uint32_t)(c - '0');
          if (lv > 255) ok = false;
        }
        if (ok) r.default_level = (uint8_t)lv;
        else r.default_level.reset();
      }
      JW w;
      fit_request_json(r, w);
      std::string o;
      if (eft_calc) {
        wk.calc_json(w.s);
        o = json_pretty(wk.out.s);
      } else {
        o = json_pretty(w.s);
      }
      o.push_back('\n');
      write_out(o);
      fflush(stdout);
      return 0;
    }
    if (cmd == "calc") {
      double tc = now_ms();
      bool ok = wk.calc_json(in);
      double te = now_ms();
      wk.out.s.push_back('\n');
      write_out(wk.out.s);
      fflush(stdout);
      if (getenv("EVEJ_TIMING"))
        fprintf(stderr, "read %.3f ms, worker %.3f ms, calc %.3f ms, write %.3f ms\n", tw - load_ms - t0, tc - tw, te - tc,
                now_ms() - te);
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
  else if (cmd == "search") search_json(*ds, rest, 20, w);
  std::string o = json_pretty(w.s);
  o.push_back('\n');
  write_out(o);
  return 0;
}
