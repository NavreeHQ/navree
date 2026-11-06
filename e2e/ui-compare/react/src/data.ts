// The screens' data, made from the text files in ../../data with integer arithmetic only.
// The Navree build and the harness make the same rows from the same files:
// ../../SCREENS.md says how.
import citiesFile from "../../data/cities.txt?raw"
import firstNamesFile from "../../data/first-names.txt?raw"
import surnamesFile from "../../data/surnames.txt?raw"

const lines = (text: string) =>
  text
    .split("\n")
    .map((line) => line.trim())
    .filter((line) => line.length > 0)

export const CITIES = lines(citiesFile)
const FIRST_NAMES = lines(firstNamesFile)
const SURNAMES = lines(surnamesFile)
const STATUSES = ["Paid", "Pending", "Overdue", "Refunded"]

export type Payment = {
  id: number
  name: string
  city: string
  rupees: number
  amount: string
  status: string
}

/** `2129334` as `₹21,29,334`: the last three digits, then groups of two. */
export function lakh(amount: number): string {
  const digits = String(amount)
  if (digits.length <= 3) return "₹" + digits
  const head = digits.slice(0, -3)
  const tail = digits.slice(-3)
  let out = ""
  for (let at = 0; at < head.length; at++) {
    if (at > 0 && (head.length - at) % 2 === 0) out += ","
    out += head[at]
  }
  return "₹" + out + "," + tail
}

export const PAYMENTS: Payment[] = Array.from({ length: 500 }, (_, at) => {
  const rupees = ((at * 7919 + 211) % 503) * 4217 + 12500
  return {
    id: at + 1,
    name: `${FIRST_NAMES[at % 40]} ${SURNAMES[(at * 7 + Math.floor(at / 40)) % 25]}`,
    city: CITIES[(at * 17) % 300],
    rupees,
    amount: lakh(rupees),
    status: STATUSES[(at * 3 + Math.floor(at / 5)) % 4],
  }
})
