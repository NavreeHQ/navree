-- GET /api/deals cycling through pages 1..50 (the bearer token comes from -H).
dofile("/scripts/report.lua")

local counter = 0

request = function()
  counter = counter + 1
  return wrk.format("GET", "/api/deals?page=" .. ((counter % 50) + 1) .. "&per_page=20")
end
