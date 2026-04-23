#include <Hypervisor/Hypervisor.h>
#include <stdbool.h>

__attribute__((weak_import))
extern hv_return_t hv_vm_config_get_el2_supported(bool *el2_supported);

__attribute__((weak_import))
extern hv_return_t hv_vm_config_set_el2_enabled(hv_vm_config_t config, bool el2_enabled);

hv_return_t sagens_hvf_vm_config_get_el2_supported(bool *el2_supported) {
    if (hv_vm_config_get_el2_supported == 0) {
        return -1;
    }
    return hv_vm_config_get_el2_supported(el2_supported);
}

hv_return_t sagens_hvf_vm_config_set_el2_enabled(hv_vm_config_t config, bool el2_enabled) {
    if (hv_vm_config_set_el2_enabled == 0) {
        return -1;
    }
    return hv_vm_config_set_el2_enabled(config, el2_enabled);
}
