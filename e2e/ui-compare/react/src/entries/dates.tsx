import { StrictMode } from "react"
import { createRoot } from "react-dom/client"

import "../styles.css"
import { DatesScreen } from "../screens/dates"

createRoot(document.getElementById("root")!).render(
  <StrictMode>
    <DatesScreen />
  </StrictMode>
)
