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
      MuiAlert: {
        styleOverrides: {
          root: {
            borderRadius: 0,
          },
        },
      },
      MuiAccordion: {
        defaultProps: {
          elevation: 0,
        },
      },
      MuiCircularProgress: {
        defaultProps: {
          disableShrink: true,
        },
      },
      MuiLinearProgress: {
        styleOverrides: {
          bar: {
            transition: "none",
          },
        },
      },
      MuiFab: {
        styleOverrides: {
          root: {
            boxShadow: "none",
          },
        },
      },
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
