import { StrictMode } from "react"
import { createRoot } from "react-dom/client"

import "../styles.css"
import { SelectScreen } from "../screens/select"

createRoot(document.getElementById("root")!).render(
  <StrictMode>
    <SelectScreen />
  </StrictMode>
)
