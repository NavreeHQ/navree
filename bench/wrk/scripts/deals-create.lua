-- POST /api/deals with a new valid deal per request (the bearer token comes from -H).
dofile("/scripts/report.lua")

wrk.headers["Content-Type"] = "application/json"

local stages = { "lead", "qualified", "proposal", "negotiation", "won", "lost" }
local counter = 0

request = function()
  counter = counter + 1
  local body = string.format(
    '{"title":"Bench deal %d","amount_cents":%d,"stage":"%s"}',
    counter, counter * 100, stages[(counter % 6) + 1])
  return wrk.format("POST", "/api/deals", nil, body)
end
