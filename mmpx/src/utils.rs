use image::Rgba;

pub fn clamp(input: i32, min: i32, max: i32) -> i32 {
    if input < min {
        min
    } else if input > max {
        max
    } else {
        input
    }
}

pub fn any_eq3(b: Rgba<u8>, a0: Rgba<u8>, a1: Rgba<u8>, a2: Rgba<u8>) -> bool {
    b == a0 || b == a1 || b == a2
}

pub fn all_eq2(b: Rgba<u8>, a0: Rgba<u8>, a1: Rgba<u8>) -> bool {
    b == a0 && b == a1
}

pub fn all_eq3(b: Rgba<u8>, a0: Rgba<u8>, a1: Rgba<u8>, a2: Rgba<u8>) -> bool {
    b == a0 && b == a1 && b == a2
}

pub fn all_eq4(b: Rgba<u8>, a0: Rgba<u8>, a1: Rgba<u8>, a2: Rgba<u8>, a3: Rgba<u8>) -> bool {
    b == a0 && b == a1 && b == a2 && b == a3
}

pub fn none_eq2(b: Rgba<u8>, a0: Rgba<u8>, a1: Rgba<u8>) -> bool {
    b != a0 && b != a1
}

pub fn none_eq4(b: Rgba<u8>, a0: Rgba<u8>, a1: Rgba<u8>, a2: Rgba<u8>, a3: Rgba<u8>) -> bool {
    b != a0 && b != a1 && b != a2 && b != a3
}
