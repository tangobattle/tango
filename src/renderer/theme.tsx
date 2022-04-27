import green from "@mui/material/colors/green";
import { createTheme as muiCreateTheme } from "@mui/material/styles";

export default function createTheme(mode: "dark" | "light") {
  return muiCreateTheme({
    typography: {
      button: {
        fontWeight: "bold",
        textTransform: "none",
      },
    },
    palette: {
      mode,
      primary: green,
    },
    components: {
      MuiButton: {
        defaultProps: {
          disableElevation: true,
        },
        styleOverrides: {
          sizeMedium: {
            height: "40px",
          },
        },
      },
      MuiTabs: {
        styleOverrides: {
          root: {
            minHeight: "0",
          },
        },
      },
      MuiTab: {
        styleOverrides: {
          root: {
            minHeight: "0",
          },
        },
      },
      MuiChip: {
        styleOverrides: {
          root: {
            borderRadius: 4,
            height: "auto",
          },
        },
      },
    },
  });
}
