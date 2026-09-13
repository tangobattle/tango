#include "Luau/Ast.h"
#include "Luau/BuiltinDefinitions.h"
#include "Luau/ConfigResolver.h"
#include "Luau/Error.h"
#include "Luau/ExperimentalFlags.h"
#include "Luau/FileResolver.h"
#include "Luau/Frontend.h"
#include "Luau/Scope.h"
#include "Luau/TypeArena.h"
#include "lua.h"

#include <cstdint>
#include <exception>
#include <cstring>
#include <mutex>
#include <string>

#include <unordered_map>
#include <unordered_set>

extern "C" int tango_luau_globals_index() noexcept { return LUA_GLOBALSINDEX; }

namespace {
struct Text { const uint8_t* data; size_t len; };
struct Source { Text name; Text source; };
using Resolve = Text (*)(void*, Text, Text, uint32_t, uint32_t);
using Emit = void (*)(void*, Text, uint32_t, uint32_t, Text);
Text text(const std::string& value) { return {reinterpret_cast<const uint8_t*>(value.data()), value.size()}; }
std::string string(Text value) { return {reinterpret_cast<const char*>(value.data), value.len}; }

struct Files : Luau::FileResolver {
    std::unordered_map<std::string, std::string> sources;
    void* context;
    Resolve resolve;
    std::optional<Luau::SourceCode> readSource(const Luau::ModuleName& name) override {
        auto it = sources.find(name);
        if (it == sources.end()) return std::nullopt;
        return Luau::SourceCode{it->second, Luau::SourceCode::Module};
    }
    std::optional<Luau::ModuleInfo> resolveModule(const Luau::ModuleInfo* from, Luau::AstExpr* expr,
                                                const Luau::TypeCheckLimits&) override {
        auto literal = expr->as<Luau::AstExprConstantString>();
        if (!from || !literal) return std::nullopt;
        std::string request(literal->value.data, literal->value.size);
        Text target = resolve(context, text(from->name), text(request), expr->location.begin.line, expr->location.begin.column);
        if (!target.data) return std::nullopt;
        return Luau::ModuleInfo{string(target)};
    }
};
}

extern "C" void tango_luau_initialize() noexcept {
    static std::once_flag initialized;
    std::call_once(initialized, [] {
        // Apply upstream analyzer defaults only to Analysis-owned flags.
        // VM/compiler experiments can emit newer bytecode than mlua supports.
        const std::unordered_set<std::string> analysisFlags = {
#include "analysis_flags.h"
        };
        for (auto* flag = Luau::FValue<bool>::list; flag; flag = flag->next)
            if (analysisFlags.count(flag->name) && !Luau::isAnalysisFlagExperimental(flag->name))
                flag->value = true;
    });
}

extern "C" void tango_luau_check(const Source* sources, size_t count, Text definitions, void* context,
                                 Resolve resolve, Emit emit) noexcept {
    auto diagnostic = [&](const Luau::TypeError& error) {
        auto message = Luau::toString(error);
        emit(context, text(error.moduleName), error.location.begin.line, error.location.begin.column, text(message));
    };
    try {
        Files files;
        files.context = context;
        files.resolve = resolve;
        for (size_t i = 0; i < count; ++i) files.sources.emplace(string(sources[i].name), string(sources[i].source));
        Luau::NullConfigResolver config;
        config.defaultConfig.mode = Luau::Mode::Strict;
        Luau::FrontendOptions options;
        options.moduleTimeLimitSec = 5.0;
        Luau::Frontend frontend(Luau::SolverMode::New, &files, &config, options);
        Luau::unfreeze(frontend.globals.globalTypes);
        Luau::registerBuiltinGlobals(frontend, frontend.globals);
        auto loaded = frontend.loadDefinitionFile(frontend.globals, frontend.globals.globalScope, string(definitions), "@tango", false);
        if (!loaded.success) {
            for (const auto& error : loaded.parseResult.errors)
                emit(context, text("@tango"), error.getLocation().begin.line, error.getLocation().begin.column, text(error.getMessage()));
            if (loaded.module) for (const auto& error : loaded.module->errors) diagnostic(error);
            emit(context, text("@tango"), 0, 0, text("host definitions failed to load"));
            return;
        }
        // Scripts own mutable capability tables; the host only reads them.
        // Derive shallow read-only contracts from the SDK so callback checking
        // permits narrower return types and omitted unused arguments. Preserve
        // the original field TypeIds, including recursive UI types.
        for (const char* name : {"Editor", "GameMode", "Telemetry"}) {
            auto binding = frontend.globals.globalScope->lookupType(name);
            auto table = binding ? Luau::get<Luau::TableType>(Luau::follow(binding->type)) : nullptr;
            if (!table) throw std::runtime_error(std::string("missing capability type: ") + name);
            auto contract = *table;
            contract.name = std::string("_TangoHost") + name;
            for (auto& [key, property] : contract.props)
                property = Luau::Property::readonly(property.readTy.value());
            auto type = frontend.globals.globalTypes.addType(std::move(contract));
            frontend.globals.globalScope->exportedTypeBindings.emplace(std::string("_TangoHost") + name, Luau::TypeFun(type));
        }
        Luau::freeze(frontend.globals.globalTypes);
        for (size_t i = 0; i < count; ++i) {
            auto result = frontend.check(string(sources[i].name));
            for (const auto& error : result.errors) diagnostic(error);
            for (const auto& name : result.timeoutHits)
                emit(context, text(name), 0, 0, text("type checking exceeded its time limit"));
        }
    } catch (const std::exception& error) {
        emit(context, text("package"), 0, 0, text(error.what()));
    } catch (...) {
        emit(context, text("package"), 0, 0, text("upstream Luau checker failed"));
    }
}
