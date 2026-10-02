// EFT fitting text import/export (contract: `eft`, rpc eft_parse / eft_export). Behaviour mirrors eve-dogma-rs src/eft.rs.
#pragma once
#include <string>
#include <string_view>

#include "dataset.hpp"
#include "jsonw.hpp"
#include "request.hpp"

namespace evej {
// EFT text -> FitRequest; returns empty string on success, else the EFT_PARSE message.
std::string eft_parse(const Dataset& ds, std::string_view text, FitRequest& out);
// FitRequest -> EFT text
std::string eft_export(const Dataset& ds, const FitRequest& req, std::string_view name);
// serde-style JSON of a FitRequest (keys sorted, every field present)
void fit_request_json(const FitRequest& r, JW& w);
// Re-indent compact JSON like serde_json::to_string_pretty
std::string json_pretty(std::string_view compact);
}  // namespace evej
