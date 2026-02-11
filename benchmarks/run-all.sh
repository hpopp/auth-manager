RESULTS_DIR="benchmarks/$(date '+%Y%m%d-%H%M%S')-results"
mkdir -p "$RESULTS_DIR"

echo "Results will be saved to $RESULTS_DIR"
echo ""

for f in benchmarks/*.js; do
  [ "$(basename "$f")" = "helpers.js" ] && continue
  name="$(basename "$f" .js)"
  echo "--- Running $name ---"
  k6 run --summary-export="${RESULTS_DIR}/${name}.json" "$f"
  echo ""
done

echo "All results saved to $RESULTS_DIR"
