#!/usr/bin/env escript
%% -*- erlang -*-
%%! -pa ./ebin

main(_) ->
    etap:plan(unknown),
    etap_can:loaded_ok(adtm_link, "Module 'adtm_link' loaded."),
    etap_can:has_attrib(adtm_link, behavior),
    etap_can:is_attrib(adtm_link, behavior, gen_server),
    etap_can:can_ok(adtm_link, create),
    etap_can:can_ok(adtm_link, create, 3),
    etap_can:can_ok(adtm_link, create, 4),
    etap_can:can_ok(adtm_link, hibern),
    etap_can:can_ok(adtm_link, hibern, 3),
    etap_can:can_ok(adtm_link, awake),
    etap_can:can_ok(adtm_link, awake, 3),
    etap_can:can_ok(adtm_link, destroy),
    etap_can:can_ok(adtm_link, destroy, 3),
    etap_can:can_ok(adtm_link, resur),
    etap_can:can_ok(adtm_link, resur, 3),
    etap_can:can_ok(adtm_link, purge),
    etap_can:can_ok(adtm_link, purge, 3),

    application:start(adtm),
    adtm_storage_server:init_storage(),

    % First create two classes
    adtm_class:create("Bridge"),
    adtm_class:create("River"),

    % Try to create an link
    etap:is(adtm_link:create("Bridge", "River", "cross"), {ok, alive}, "Creating Link Bridge-cross-River"),

    % Changing the adt state
    etap:is(adtm_link:hibern("Bridge", "River", "cross"), {ok, frozen}, "Freezing Link Bridge-cross-River"),
    etap:is(adtm_link:awake("Bridge", "River", "cross"), {ok, alive}, "Unfreezing Link Bridge-cross-River"),
    etap:is(adtm_link:destroy("Bridge", "River", "cross"), {ok, destroyed}, "Destroying Link Bridge-cross-River"),
    etap:is(adtm_link:resur("Bridge", "River", "cross"), {ok, alive}, "Resurecting Link Bridge-cross-River"),

    % Try to create an link in a class that doesnt exist
    etap:is(adtm_link:create("Nope", "River", "cross"), {error, invalid_parents}, "Can't create Nope-cross-River"),
    etap:is(adtm_link:create("Bridge", "Nope", "cross"), {error, invalid_parents}, "Can't create Bridge-cross-Nope"),
    etap:is(adtm_link:create("Nope", "Nope", "cross"), {error, invalid_parents}, "Can't create Nope-cross-Nope"),

    % Try to recreate the same link in the same class
    etap:is(adtm_link:create("Bridge", "River", "cross"), {error, already_exists}, "Can't create the same link twice"),

    application:stop(adtm),

    etap:end_tests(),
    ok.

