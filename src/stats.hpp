#pragma once
#include <optional>

#include "engine.hpp"
#include "jsonw.hpp"

namespace evej {
inline constexpr const char* ENGINE_NAME = "eve-dogma-j 0.1.0";
double range_factor(double optimal, double falloff, std::optional<double> distance, bool restricted);
void compute_stats(Fit& fit, const FitRequest& req, JW& out);
}  // namespace evej
