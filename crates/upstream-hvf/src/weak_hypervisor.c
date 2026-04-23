#include <stdbool.h>
#include <stddef.h>
#include <stdint.h>

typedef int hv_return_t;
typedef struct hv_vm_config_s *hv_vm_config_t;
typedef struct hv_gic_config_s *hv_gic_config_t;
typedef uint64_t hv_ipa_t;

enum {
    SAGENS_HVF_SYMBOL_MISSING = -1,
    SAGENS_HVF_SYMBOL_FAILED = -2,
};

__attribute__((weak_import))
extern hv_return_t hv_vm_config_get_el2_supported(bool *el2_supported);

__attribute__((weak_import))
extern hv_return_t hv_vm_config_set_el2_enabled(hv_vm_config_t config, bool el2_enabled);

__attribute__((weak_import))
extern hv_gic_config_t hv_gic_config_create(void);

__attribute__((weak_import))
extern hv_return_t hv_gic_config_set_distributor_base(hv_gic_config_t config, hv_ipa_t addr);

__attribute__((weak_import))
extern hv_return_t hv_gic_config_set_redistributor_base(hv_gic_config_t config, hv_ipa_t addr);

__attribute__((weak_import))
extern hv_return_t hv_gic_create(hv_gic_config_t config);

__attribute__((weak_import))
extern hv_return_t hv_gic_get_distributor_size(size_t *dist_size);

__attribute__((weak_import))
extern hv_return_t hv_gic_get_redistributor_size(size_t *redist_size);

__attribute__((weak_import))
extern hv_return_t hv_gic_set_spi(uint32_t intid, bool asserted);

hv_return_t sagens_hvf_vm_config_get_el2_supported(bool *el2_supported) {
    if (hv_vm_config_get_el2_supported == 0) {
        return SAGENS_HVF_SYMBOL_MISSING;
    }
    return hv_vm_config_get_el2_supported(el2_supported);
}

hv_return_t sagens_hvf_vm_config_set_el2_enabled(hv_vm_config_t config, bool el2_enabled) {
    if (hv_vm_config_set_el2_enabled == 0) {
        return SAGENS_HVF_SYMBOL_MISSING;
    }
    return hv_vm_config_set_el2_enabled(config, el2_enabled);
}

hv_return_t sagens_hvf_gic_config_create(hv_gic_config_t *config) {
    if (hv_gic_config_create == 0) {
        return SAGENS_HVF_SYMBOL_MISSING;
    }
    if (config == 0) {
        return SAGENS_HVF_SYMBOL_FAILED;
    }

    *config = hv_gic_config_create();
    return *config != 0 ? 0 : SAGENS_HVF_SYMBOL_FAILED;
}

hv_return_t sagens_hvf_gic_config_set_distributor_base(hv_gic_config_t config, hv_ipa_t addr) {
    if (hv_gic_config_set_distributor_base == 0) {
        return SAGENS_HVF_SYMBOL_MISSING;
    }
    return hv_gic_config_set_distributor_base(config, addr);
}

hv_return_t sagens_hvf_gic_config_set_redistributor_base(hv_gic_config_t config, hv_ipa_t addr) {
    if (hv_gic_config_set_redistributor_base == 0) {
        return SAGENS_HVF_SYMBOL_MISSING;
    }
    return hv_gic_config_set_redistributor_base(config, addr);
}

hv_return_t sagens_hvf_gic_create(hv_gic_config_t config) {
    if (hv_gic_create == 0) {
        return SAGENS_HVF_SYMBOL_MISSING;
    }
    return hv_gic_create(config);
}

hv_return_t sagens_hvf_gic_get_distributor_size(size_t *dist_size) {
    if (hv_gic_get_distributor_size == 0) {
        return SAGENS_HVF_SYMBOL_MISSING;
    }
    return hv_gic_get_distributor_size(dist_size);
}

hv_return_t sagens_hvf_gic_get_redistributor_size(size_t *redist_size) {
    if (hv_gic_get_redistributor_size == 0) {
        return SAGENS_HVF_SYMBOL_MISSING;
    }
    return hv_gic_get_redistributor_size(redist_size);
}

hv_return_t sagens_hvf_gic_set_spi(uint32_t intid, bool asserted) {
    if (hv_gic_set_spi == 0) {
        return SAGENS_HVF_SYMBOL_MISSING;
    }
    return hv_gic_set_spi(intid, asserted);
}
