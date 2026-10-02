# Golden regression test: run one request through the engine and compare with the stored output byte for byte.
# The stored outputs were produced by eve-dogma-j and checked byte-identical against eve-dogma-rs (engine name
# aside; BAD_REQUEST message wording excepted). -DBIN=<eve-dogma-j> -DMODE=calc|serve-stdio -DIN=<request file> -DEXP=<expected file>
# Dataset: $EVE_DOGMA_DATASET (or the engine's own default lookup).
execute_process(COMMAND ${BIN} ${MODE} INPUT_FILE ${IN} OUTPUT_VARIABLE out ERROR_VARIABLE err RESULT_VARIABLE rc)
file(READ ${EXP} exp)
string(STRIP "${out}" out)
string(STRIP "${exp}" exp)
if(NOT out STREQUAL exp)
  string(SUBSTRING "${out}" 0 400 head)
  message(FATAL_ERROR "output differs from ${EXP} (rc ${rc}) ${err}\n${head}")
endif()
