mkdir -p benchmarks/results

for f in benchmarks/*.js; do
  [ "$(basename "$f")" = "helpers.js" ] && continue
  name="$(basename "$f" .js)"
  echo "--- Running $name ---"
  k6 run --summary-export="benchmarks/results/${name}.json" "$f"
  echo ""
done
