#include "sheetsage-events.h"
#include "lua-plugin.h"

#include <cmath>
#include <cstdio>
#include <cstdlib>
#include <limits>

int main() {
    const struct {
        double value;
        const char * text;
    } cases[] = {
        {-0.0, "-0.0"},
        {0.1, "0.1"},
        {-123.125, "-123.125"},
        {0.0001, "0.0001"},
        {0.00001, "1e-05"},
        {1e15, "1000000000000000.0"},
        {1e16, "1e+16"},
        {0.1 + 0.2, "0.30000000000000004"},
        {std::numeric_limits<double>::denorm_min(), "5e-324"},
        {std::numeric_limits<double>::max(), "1.7976931348623157e+308"},
    };
    for (const auto & test : cases) {
        const auto text = yue2_sheet_python_repr_double(test.value);
        char * end = nullptr;
        const double parsed = std::strtod(text.c_str(), &end);
        if (text != test.text || *end != '\0' || parsed != test.value ||
            std::signbit(parsed) != std::signbit(test.value)) {
            std::fprintf(stderr, "Expected %s, got %s\n", test.text, text.c_str());
            return 1;
        }
    }
    for (const auto & test : cases) {
        const auto text = lua_js_number_string(test.value);
        char * end = nullptr;
        const double parsed = std::strtod(text.c_str(), &end);
        if (*end != '\0' || parsed != test.value) {
            std::fprintf(stderr, "Lua numeric option failed to round-trip: %s\n", text.c_str());
            return 1;
        }
    }
    if (lua_js_number_string(-0.0) != "0" || lua_js_number_string(1e-5) != "0.00001" ||
        lua_js_number_string(1e21) != "1e+21") {
        std::fputs("Lua numeric option differs from ECMAScript formatting\n", stderr);
        return 1;
    }
    std::puts("SheetSage and Lua floats preserve decimal text and round-trip");
}
